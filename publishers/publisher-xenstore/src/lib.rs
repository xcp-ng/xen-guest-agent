mod rfc;
mod std;
mod version;

use ::std::io;

use guest_metrics::plugin::GuestAgentPublisher;
use xenstore_rs::{AsyncWatch, AsyncXs, Xs};
use xenstore_win::smol::XsSmolWindows;

pub fn xs_publish(xs: &impl Xs, key: &str, value: &str) -> io::Result<()> {
    log::trace!("+ {}={:?}", key, value);
    xs.write(key, value)
}

pub async fn xs_publish_async(xs: &impl AsyncXs, key: &str, value: &str) -> io::Result<()> {
    log::trace!("+ {}={:?}", key, value);
    xs.write(key, value).await
}

pub fn xs_unpublish(xs: &impl Xs, key: &str) -> io::Result<()> {
    log::trace!("- {}", key);
    xs.rm(key)
}

pub async fn xs_unpublish_async(xs: &impl AsyncXs, key: &str) -> io::Result<()> {
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
    async fn run(self, channel: flume::Receiver<guest_metrics::GuestMetric>) {
        #[cfg(not(target_os = "windows"))]
        let xs = xenstore_rs::unix::XsUnix::new().expect("Unable to initialize xenstore");

        #[cfg(target_os = "windows")]
        let xs = xenstore_win::XsWindows::new().expect("Unable to initialize xenstore");

        rfc::XenstoreRfc::new(xs)
            .run(channel)
            .await
            .expect("Xenstore failure")
    }
}

pub struct XenstoreStdPublisher;

impl GuestAgentPublisher for XenstoreStdPublisher {
    async fn run(self, channel: flume::Receiver<guest_metrics::GuestMetric>) {
        #[cfg(not(target_os = "windows"))]
        let xs = xenstore_rs::unix::XsUnix::new().expect("Unable to initialize xenstore");

        #[cfg(target_os = "windows")]
        let xs = XsSmolWindows::new()
            .await
            .expect("Unable to initialize xenstore");

        std::XenstoreStd::new(xs)
            .run(channel)
            .await
            .expect("Xenstore failure")
    }
}
