use std::{io, time::Duration};

use futures::StreamExt;
use tokio::sync::mpsc;

use crate::{
    provider::{kernel::collect_kernel, memory::MemorySource, net::NetworkSource},
    publisher::{GuestMetric, MemoryInfo, OsInfo},
    GuestAgentConfig,
};

pub async fn run(
    config: GuestAgentConfig,
    publisher_channel: mpsc::Sender<GuestMetric>,
    mut collector_memory: impl MemorySource + Send + 'static,
    mut collector_net: impl NetworkSource + Send + Unpin + 'static,
) -> anyhow::Result<()> {
    // Remove old entries from previous agent to avoid having unknown
    // interfaces. We will repopulate existing ones immediatly.
    publisher_channel.send(GuestMetric::CleanupIfaces).await?;

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
    publisher_channel
        .send(GuestMetric::OsInfo(OsInfo {
            os_info: os_info::get(),
            kernel_info,
        }))
        .await?;

    // network events
    for event in collector_net.collect_current().await? {
        if config.report_nics {
            publisher_channel.send(GuestMetric::Network(event)).await?;
        }
    }

    // main loop
    let network_task = tokio::spawn({
        let publisher_channel = publisher_channel.clone();
        async move {
            loop {
                while let Some(events) = collector_net.next().await {
                    for event in events {
                        publisher_channel
                            .send(GuestMetric::Network(event))
                            .await
                            .unwrap();
                    }
                }
            }
        }
    });

    let memory_task = tokio::spawn({
        let publisher_channel = publisher_channel.clone();
        let mut timer = tokio::time::interval(Duration::from_secs_f64(config.period));

        async move {
            loop {
                timer.tick().await;
                let mem_total = mem_total.unwrap_or_default();
                let mem_free = collector_memory.get_available_kb().unwrap();

                publisher_channel
                    .send(GuestMetric::MemoryInfo(MemoryInfo {
                        mem_free,
                        mem_total,
                    }))
                    .await
                    .unwrap();
            }
        }
    });

    let _ = futures::join!(network_task, memory_task);

    Ok(())
}
