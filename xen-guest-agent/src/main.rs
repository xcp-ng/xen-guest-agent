mod plugins;
mod publisher;
#[cfg(windows)]
mod windows_debug_logger;
#[cfg(windows)]
mod windows_service_main;

use clap::Parser;
use log::LevelFilter;
use smol::Executor;

use guest_metrics::{plugin::GuestAgentPlugin, GuestMetric};
use plugins::{NetworkPlugin, NetworkPluginKind};
use publisher::{AgentPublisher, PublisherKind};

#[cfg(windows)]
use windows_debug_logger::WindowsDebugLogger;

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

    /// Run as a Windows service.
    #[cfg(windows)]
    #[arg(long)]
    service: bool,
}

pub(crate) async fn run_async(
    executor: &Executor<'_>,
    config: &GuestAgentConfig,
) -> anyhow::Result<()> {
    setup_logger(config.stderr, config.log_level)?;

    let (tx, rx) = flume::bounded(4);
    let publisher = AgentPublisher::new(config.publisher)?;
    let mut tasks = vec![];

    tasks.push(executor.spawn(publisher.run(rx)));

    if config.report_nics {
        // Remove old entries from previous agent to avoid having unknown
        // interfaces. We will repopulate existing ones immediatly.
        tx.send_async(GuestMetric::CleanupIfaces).await?;
        tasks.push(executor.spawn(NetworkPlugin::new(config.network)?.run(tx.clone())));
    }

    tasks.push(executor.spawn(provider_os::OsInfoPlugin.run(tx.clone())));
    tasks.push(executor.spawn(provider_memory::MemoryPlugin.run(tx.clone())));

    for task in tasks {
        task.await
    }

    Ok(())
}

#[cfg(unix)]
fn main() -> anyhow::Result<()> {
    let config = GuestAgentConfig::parse();
    let executor = Executor::new();

    smol::block_on(executor.run(run_async(&executor, &config)))
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    let config = GuestAgentConfig::parse();
    if config.service {
        windows_service_main::dispatch_main()
    } else {
        let executor = Executor::new();
        smol::block_on(executor.run(run_async(&executor, &config)))
    }
}

fn setup_logger(use_stderr: bool, level: LevelFilter) -> anyhow::Result<()> {
    if use_stderr {
        setup_env_logger(level)?;
    } else {
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

#[cfg(windows)]
fn setup_system_logger(level: LevelFilter) -> anyhow::Result<()> {
    log::set_boxed_logger(Box::new(WindowsDebugLogger {}))?;
    log::set_max_level(level);
    Ok(())
}
