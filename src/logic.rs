use std::{io, time::Duration};

use futures::{pin_mut, select, FutureExt, TryStreamExt};

use crate::{
    provider::{kernel::collect_kernel, memory::MemorySource, net::NetworkSource},
    publisher::{MemoryInfo, OsInfo, Publisher},
    GuestAgentConfig,
};

pub async fn run(
    config: GuestAgentConfig,
    mut publisher: impl Publisher,
    mut collector_memory: impl MemorySource,
    mut collector_net: impl NetworkSource,
) -> anyhow::Result<()> {
    // Remove old entries from previous agent to avoid having unknown
    // interfaces. We will repopulate existing ones immediatly.
    publisher.cleanup_ifaces()?;

    let kernel_info = collect_kernel()?;
    let mem_total = match collector_memory.get_total_kb() {
        Ok(mem_total_kb) => Some(mem_total_kb),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            log::warn!("Memory stats not supported");
            None
        }
        // propagate errors other than io::ErrorKind::Unsupported
        Err(error) => Err(error)?,
    };
    publisher.publish_osinfo(&OsInfo {
        os_info: os_info::get(),
        kernel_info,
    })?;

    // periodic memory stat
    let mut timer_stream = tokio::time::interval(Duration::from_secs_f64(config.period));

    // network events
    for event in collector_net.collect_current().await? {
        if config.report_nics {
            publisher.publish_netevent(&event)?;
        }
    }
    let netevent_stream = collector_net.stream();
    pin_mut!(netevent_stream); // needed for iteration

    // main loop
    loop {
        select! {
            event = netevent_stream.try_next().fuse() => {
                match event? {
                    Some(event) => {
                        if config.report_nics {
                            publisher.publish_netevent(&event)?;
                        } else {
                            log::debug!("no toolstack iface in {event:?}");
                        }
                    },
                    // FIXME can't we handle those in `select!` directly?
                    None => { /* closed? */ },
                };
            },
            _ = timer_stream.tick().fuse() => {
              let mem_total = mem_total.unwrap_or_default();
                match collector_memory.get_available_kb() {
                    Ok(mem_avail_kb) => publisher.publish_memory(&MemoryInfo {
                      mem_total,
                      mem_free: mem_total - mem_avail_kb
                    })?,
                    Err(e) if e.kind() == io::ErrorKind::Unsupported => (),
                    Err(e) => Err(e)?,
                }
            },
            complete => break,
        }
    }

    Ok(())
}
