use std::{sync::Arc, usize};

use futures::StreamExt;
use guest_metrics::{
    plugin::{GuestAgentPlugin, Shared},
    GuestMetric,
};
use xenstore_rs::AsyncWatch;

pub enum LiveMigrationDetect {
    None,
    /// Watch based live migration detection
    XenStore,
    /// Windows driver based live migration detection
    #[cfg(target_family = "windows")]
    Windows,
}

impl LiveMigrationDetect {
    /// Detect live migration using a sentinel watch that is expected to be
    /// triggered at the next resume (as all watches are supposed to be
    /// recreated, and a watch is triggered at creation).
    async fn xenstore(&self, shared: Arc<Shared>) {
        let Some(xs) = &shared.xs else {
            log::warn!("xenstore is not available, can't detect live migrations");
            return;
        };

        log::info!("Using xenstore live migration detection method");

        let mut watch = xs
            .watch("xs-rs-suspend-sentinel")
            .await
            .expect("Unable to create sentinel watch");

        watch.next().await;

        loop {
            watch.next().await;
            log::info!("Detected live migration");
            shared.live_migration_event.notify(usize::MAX);
        }
    }
}

impl GuestAgentPlugin for LiveMigrationDetect {
    async fn run(self, shared: Arc<Shared>, _: flume::Sender<GuestMetric>) {
        match self {
            Self::None => {}
            Self::XenStore => self.xenstore(shared).await,
            #[cfg(target_family = "windows")]
            Self::Windows => todo!(),
        }
    }
}
