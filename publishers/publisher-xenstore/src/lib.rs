mod rfc;
mod std;

use ::std::{io, sync::Arc};

use guest_metrics::plugin::GuestAgentPublisher;
use xenstore_rs::{AsyncWatch, AsyncXs};

pub async fn xs_publish(xs: &impl AsyncXs, key: &str, value: &str) -> io::Result<()> {
    log::trace!("+ {}={:?}", key, value);
    xs.write(key, value).await
}

pub async fn xs_unpublish(xs: &impl AsyncXs, key: &str) -> io::Result<()> {
    log::trace!("- {}", key);
    xs.rm(key).await
}

pub async fn xs_watch_oneshot_async(xs: &impl AsyncWatch, key: &str) -> io::Result<()> {
    log::trace!("? {}", key);
    let s = xs.watch(key).await;
    s.iter().next();
    Ok(())
}

pub struct XenstoreRfcPublisher;

impl GuestAgentPublisher for XenstoreRfcPublisher {
    async fn run(
        self,
        shared: Arc<guest_metrics::plugin::Shared>,
        channel: flume::Receiver<guest_metrics::GuestMetric>,
    ) {
        let xs = shared.xs.clone().expect("xenstore is not available");

        rfc::XenstoreRfc::new(xs)
            .run(channel)
            .await
            .expect("Xenstore failure")
    }
}

pub struct XenstoreStdPublisher;

impl GuestAgentPublisher for XenstoreStdPublisher {
    async fn run(
        self,
        shared: Arc<guest_metrics::plugin::Shared>,
        channel: flume::Receiver<guest_metrics::GuestMetric>,
    ) {
        let xs = shared.xs.clone().expect("xenstore is not available");

        std::XenstoreStd::new(xs)
            .run(channel)
            .await
            .expect("Xenstore failure")
    }
}
