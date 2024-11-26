mod datastructs;

mod collector;
mod publisher;
mod vif_detect;

use clap::Parser;
use collector::memory::{MemorySource, PlatformMemorySource};
use collector::net::{NetworkSource, PlatformNetworkSource};
use publisher::{AgentPublisher, Publisher};
use datastructs::KernelInfo;

use futures::{pin_mut, select, FutureExt, TryStreamExt};
use std::cell::LazyCell;
use std::error::Error;
use std::io;
use std::str::FromStr;
use std::time::Duration;

const REPORT_INTERNAL_NICS: bool = false; // FIXME make this a CLI flag
const MEM_PERIOD_SECONDS: u64 = 60;
const DEFAULT_LOGLEVEL: &str = "info";

//TODO: Shouldn't be like that
struct LazyXs<XS: xenstore_rs::Xs>(LazyCell<XS>);

impl<XS: xenstore_rs::Xs> xenstore_rs::Xs for LazyXs<XS> {
    fn directory(&self, path: &str) -> io::Result<Vec<Box<str>>> {
        self.0.directory(path)
    }

    fn read(&self, path: &str) -> io::Result<Box<str>> {
        self.0.read(path)
    }

    fn write(&self, path: &str, data: &str) -> io::Result<()> {
        self.0.write(path, data)
    }

    fn rm(&self, path: &str) -> io::Result<()> {
        self.0.rm(path)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    setup_logger(cli.stderr, &cli.loglevel)?;

    let mut publisher = AgentPublisher::new(LazyXs(LazyCell::new(|| {
        xenstore_rs::unix::XsUnix::new().expect("Xenstore not available")
    })))?;

    let mut collector_memory = PlatformMemorySource::new()?;

    // Remove old entries from previous agent to avoid having unknown
    // interfaces. We will repopulate existing ones immediatly.
    publisher.cleanup_ifaces()?;

    let kernel_info = collect_kernel()?;
    let mem_total_kb = match collector_memory.get_total_kb() {
        Ok(mem_total_kb) => Some(mem_total_kb),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            log::warn!("Memory stats not supported");
            None
        }
        // propagate errors other than io::ErrorKind::Unsupported
        Err(error) => Err(error)?,
    };
    publisher.publish_static(&os_info::get(), &kernel_info, mem_total_kb)?;

    // periodic memory stat
    let mut timer_stream = tokio::time::interval(Duration::from_secs(MEM_PERIOD_SECONDS));

    // network events
    let mut collector_net = PlatformNetworkSource::new()?;
    for event in collector_net.collect_current().await? {
        if REPORT_INTERNAL_NICS {
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
                        if REPORT_INTERNAL_NICS {
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
                match collector_memory.get_available_kb() {
                    Ok(mem_avail_kb) => publisher.publish_memfree(mem_avail_kb)?,
                    Err(ref e) if e.kind() == io::ErrorKind::Unsupported => (),
                    Err(e) => Err(e)?,
                }
            },
            complete => break,
        }
    }

    Ok(())
}

#[derive(clap::Parser)]
struct Cli {
    /// Print logs to stderr instead of system logs
    #[arg(short, long)]
    stderr: bool,

    /// Highest level of detail to log
    #[arg(short, long, default_value_t = String::from(DEFAULT_LOGLEVEL))]
    loglevel: String,
}

fn setup_logger(use_stderr: bool, loglevel_string: &str) -> Result<(), Box<dyn Error>> {
    if use_stderr {
        setup_env_logger(loglevel_string)?;
    } else {
        #[cfg(not(unix))]
        panic!("no system logger supported");

        #[cfg(unix)]
        setup_system_logger(loglevel_string)?;
    }
    Ok(())
}

// stdout logger for platforms with no specific implementation
fn setup_env_logger(loglevel_string: &str) -> Result<(), Box<dyn Error>> {
    // set default threshold to "info" not "error"
    let env = env_logger::Env::default().default_filter_or(loglevel_string);
    env_logger::Builder::from_env(env).init();
    Ok(())
}

#[cfg(unix)]
// syslog logger
fn setup_system_logger(loglevel_string: &str) -> Result<(), Box<dyn Error>> {
    let formatter = syslog::Formatter3164 {
        facility: syslog::Facility::LOG_USER,
        hostname: None,
        process: env!("CARGO_PKG_NAME").into(),
        pid: 0,
    };

    let logger = match syslog::unix(formatter) {
        Err(e) => {
            eprintln!("impossible to connect to syslog: {:?}", e);
            return Ok(());
        }
        Ok(logger) => logger,
    };
    log::set_boxed_logger(Box::new(syslog::BasicLogger::new(logger)))?;
    log::set_max_level(log::LevelFilter::from_str(loglevel_string)?);
    Ok(())
}

// UNIX uname() implementation
#[cfg(unix)]
fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    let uname_info = uname::uname()?;
    let info = KernelInfo {
        release: uname_info.release,
    };

    Ok(Some(info))
}

// default implementation
#[cfg(not(unix))]
fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    Ok(None)
}
