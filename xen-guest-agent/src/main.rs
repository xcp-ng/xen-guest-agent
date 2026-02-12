mod plugins;
mod publisher;
#[cfg(windows)]
mod windows_service_main;

use std::sync::Arc;

use clap::Parser;
use event_listener::Event;
use flume::Receiver;
use futures::future::{join_all, select};
use log::LevelFilter;

use guest_metrics::{
    plugin::{GuestAgentPlugin, Shared},
    GuestMetric,
};
use plugins::{NetworkPlugin, NetworkPluginKind};
use publisher::{AgentPublisher, PublisherKind};

use smol::Executor;
#[cfg(windows)]
use xen_win_utils::windows_debug_logger::WindowsDebugLogger;
use xenstore_rs::smol::XsSmol;

use crate::plugins::{build_platform_vif_detector, VifDetectionMethod};

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

    /// Method to use to idenfity vifs
    #[arg(long, value_enum, default_value_t = Default::default())]
    vif_detect: VifDetectionMethod,

    /// Run as a Windows service.
    #[cfg(windows)]
    #[arg(long)]
    service: bool,
}

async fn build_shared(config: &GuestAgentConfig) -> Arc<Shared> {
    let executor: Executor<'static> = Executor::new();
    let xs = XsSmol::new(&executor)
        .await
        .inspect_err(|e| log::warn!("xenstore is not available: {e}"))
        .ok();

    Arc::new(Shared {
        live_migration_event: Event::new(),
        executor,
        vif_detector: build_platform_vif_detector(config.vif_detect, xs.clone()),
        xs,
    })
}

pub(crate) async fn run_async(
    config: &GuestAgentConfig,
    stop_rx: Receiver<()>,
) -> anyhow::Result<()> {
    let (tx, rx) = flume::bounded(4);
    let publisher = AgentPublisher::new(config.publisher)?;
    let mut tasks = vec![];
    let shared = build_shared(config).await;
    let executor = &shared.executor;

    tasks.push(executor.spawn(
        live_migration_detect::LiveMigrationDetect::XenStore.run(shared.clone(), tx.clone()),
    ));
    tasks.push(executor.spawn(publisher.run(shared.clone(), rx.clone())));

    if config.report_nics {
        // Remove old entries from previous agent to avoid having unknown
        // interfaces. We will repopulate existing ones immediatly.
        tx.send_async(GuestMetric::CleanupIfaces).await?;
        tasks.push(
            executor.spawn(NetworkPlugin::new(config.network)?.run(shared.clone(), tx.clone())),
        );
    }

    tasks.push(executor.spawn(provider_os::OsInfoPlugin.run(shared.clone(), tx.clone())));
    tasks.push(executor.spawn(provider_memory::MemoryPlugin.run(shared.clone(), tx.clone())));

    #[cfg(windows)]
    tasks.push(executor.spawn(
        provider_clipboard::windows::WindowsClipboardPlugin::new()?.run(shared.clone(), tx.clone()),
    ));

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
    let config = GuestAgentConfig::parse();
    setup_logger(config.stderr, config.log_level)?;

    let (_stop_tx, stop_rx) = flume::bounded(0);
    smol::block_on(run_async(&config, stop_rx))
}

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    let config = GuestAgentConfig::parse();
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
