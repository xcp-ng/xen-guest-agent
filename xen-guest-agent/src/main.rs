mod plugins;
mod publisher;
#[cfg(windows)]
mod windows_service_main;

use flume::Receiver;
use futures::future::{join_all, select};
use log::LevelFilter;
use smol::Executor;

use guest_metrics::{plugin::GuestAgentPlugin, GuestMetric};
use plugins::{NetworkPlugin, NetworkPluginKind};
use publisher::{AgentPublisher, PublisherKind};

#[cfg(windows)]
use xen_win_utils::windows_debug_logger::WindowsDebugLogger;

/// Xen Guest Agent
#[derive(argh::FromArgs)]
struct GuestAgentConfig {
    /// print logs to stderr instead of system logs
    #[argh(switch, short = 's')]
    stderr: bool,

    /// highest level of detail to log
    #[argh(option, short = 'l', default = "LevelFilter::Info")]
    log_level: LevelFilter,

    /// whether we don't report NICs
    #[argh(switch, long = "no-nics")]
    no_nics: bool,

    /// data publisher to use
    #[argh(option, default = "Default::default()")]
    publisher: PublisherKind,

    /// network plugin to use
    #[argh(option, default = "Default::default()")]
    network: NetworkPluginKind,

    /// run as a Windows service
    #[cfg(windows)]
    #[argh(switch, long = "service")]
    service: bool,
}

pub(crate) async fn run_async(
    config: &GuestAgentConfig,
    stop_rx: Receiver<()>,
) -> anyhow::Result<()> {
    let (tx, rx) = flume::bounded(4);
    let publisher = AgentPublisher::new(config.publisher)?;
    let mut tasks = vec![];
    let executor = Executor::new();

    tasks.push(executor.spawn(publisher.run(rx.clone())));

    if !config.no_nics {
        // Remove old entries from previous agent to avoid having unknown
        // interfaces. We will repopulate existing ones immediatly.
        tx.send_async(GuestMetric::CleanupIfaces).await?;
        tasks.push(executor.spawn(NetworkPlugin::new(config.network)?.run(tx.clone())));
    }

    tasks.push(executor.spawn(provider_os::OsInfoPlugin.run(tx.clone())));
    tasks.push(executor.spawn(provider_memory::MemoryPlugin.run(tx.clone())));

    #[cfg(windows)]
    tasks.push(
        executor.spawn(provider_clipboard::windows::WindowsClipboardPlugin::new()?.run(tx.clone())),
    );

    executor
        .run(async {
            log::info!("Waiting for exit command");
            select(join_all(tasks), stop_rx.recv_async()).await;
            log::info!("Got exit command");
            anyhow::Ok(())
        })
        .await?;

    Ok(())
}

#[cfg(unix)]
fn main() -> anyhow::Result<()> {
    let config: GuestAgentConfig = argh::from_env();
    setup_logger(config.stderr, config.log_level)?;

    let (_stop_tx, stop_rx) = flume::bounded(0);
    smol::block_on(run_async(&config, stop_rx))
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    let config: GuestAgentConfig = argh::from_env();
    setup_logger(config.stderr, config.log_level)?;

    if config.service {
        windows_service_main::dispatch_main()
    } else {
        let (_stop_tx, stop_rx) = flume::bounded(0);
        smol::block_on(run_async(&config, stop_rx))
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
    log::set_boxed_logger(Box::new(WindowsDebugLogger {
        prefix: "[xen-guest-agent]".to_string(),
    }))?;
    log::set_max_level(level);
    Ok(())
}
