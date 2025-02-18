mod plugins;
mod publisher;

use clap::Parser;
use futures::{channel::mpsc, SinkExt};
use log::LevelFilter;
use tokio::task::JoinSet;

use guest_metrics::{plugin::GuestAgentPlugin, GuestMetric};
use plugins::{NetworkPlugin, NetworkPluginKind};
use publisher::{AgentPublisher, PublisherKind};

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
    network: NetworkPluginKind,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = GuestAgentConfig::parse();

    setup_logger(config.stderr, config.log_level)?;

    let mut set: JoinSet<()> = JoinSet::new();

    let (mut tx, rx) = mpsc::channel(4);
    let publisher = AgentPublisher::new(config.publisher)?;

    set.spawn(publisher.run(rx));

    // Remove old entries from previous agent to avoid having unknown
    // interfaces. We will repopulate existing ones immediatly.
    tx.send(GuestMetric::CleanupIfaces).await?;

    set.spawn(provider_os::OsInfoPlugin.run(tx.clone()));
    set.spawn(provider_memory::MemoryPlugin.run(tx.clone()));
    set.spawn(NetworkPlugin::new(config.network)?.run(tx.clone()));

    println!("{:?}", set.join_all().await);
    Ok(())
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
