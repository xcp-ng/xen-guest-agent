mod datastructs;

mod logic;
mod provider;
mod publisher;
mod vif_detect;
//pub mod metrics;

use clap::Parser;
use log::LevelFilter;
use provider::memory::{MemorySource, PlatformMemorySource};
use provider::net::{AgentNetworkSource, NetworkSourceKind};
use publisher::xenstore::PlatformXs;
use publisher::PublisherKind;

const MEM_PERIOD_SECONDS: f64 = 5.0;

#[derive(clap::Parser)]
struct GuestAgentConfig {
    /// Print logs to stderr instead of system logs
    #[arg(short, long)]
    stderr: bool,

    /// Highest level of detail to log
    #[arg(short, long, default_value_t = LevelFilter::Info)]
    log_level: LevelFilter,

    /// Whether we report NICs.
    #[arg(short, long, default_value_t = true)]
    report_nics: bool,

    /// Update period.
    #[arg(short, long, default_value_t = MEM_PERIOD_SECONDS)]
    period: f64,

    #[arg(long, value_enum, default_value_t = Default::default())]
    publisher: PublisherKind,

    #[arg(long, value_enum, default_value_t = Default::default())]
    network: NetworkSourceKind,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = GuestAgentConfig::parse();

    setup_logger(config.stderr, config.log_level)?;

    let publisher_channel: tokio::sync::mpsc::Sender<publisher::GuestMetric> =
        publisher::spawn_publisher::<PlatformXs>(config.publisher)
            .expect("Unable to initialize publisher");
    let collector_memory = PlatformMemorySource::new().expect("Unable to initialize memory source");
    let collector_net =
        AgentNetworkSource::new(config.network).expect("Unable to initialize network source");

    logic::run(config, publisher_channel, collector_memory, collector_net).await
}

fn setup_logger(use_stderr: bool, level: LevelFilter) -> anyhow::Result<()> {
    if use_stderr {
        setup_env_logger(level)?;
    } else {
        #[cfg(not(unix))]
        panic!("no system logger supported");

        #[cfg(unix)]
        setup_system_logger(level)?;
    }
    Ok(())
}

// stdout logger for platforms with no specific implementation
fn setup_env_logger(level: LevelFilter) -> anyhow::Result<()> {
    // set default threshold to "info" not "error"
    let env = env_logger::Env::default().default_filter_or(level.as_str());
    env_logger::Builder::from_env(env).init();
    Ok(())
}

#[cfg(unix)]
// syslog logger
fn setup_system_logger(level: LevelFilter) -> anyhow::Result<()> {
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
    log::set_max_level(level);
    Ok(())
}
