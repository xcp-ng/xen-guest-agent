mod rfc;
mod std;

use ::std::io;

use futures::channel::mpsc;
use guest_metrics::plugin::GuestAgentPublisher;
use xenstore_rs::Xs;

pub fn xs_publish(xs: &impl Xs, key: &str, value: &str) -> io::Result<()> {
    log::trace!("+ {}={:?}", key, value);
    xs.write(key, value)
}

pub fn xs_unpublish(xs: &impl Xs, key: &str) -> io::Result<()> {
    log::trace!("- {}", key);
    xs.rm(key)
}

pub struct XenstoreRfcPublisher;

impl GuestAgentPublisher for XenstoreRfcPublisher {
    fn run(
        self,
        channel: mpsc::Receiver<guest_metrics::GuestMetric>,
    ) -> impl ::std::future::Future<Output = ()> + Send {
        async move {
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
}

pub struct XenstoreStdPublisher;

impl GuestAgentPublisher for XenstoreStdPublisher {
    fn run(
        self,
        channel: mpsc::Receiver<guest_metrics::GuestMetric>,
    ) -> impl ::std::future::Future<Output = ()> + Send {
        async move {
            #[cfg(not(target_os = "windows"))]
            let xs = xenstore_rs::unix::XsUnix::new().expect("Unable to initialize xenstore");

            #[cfg(target_os = "windows")]
            let xs = xenstore_win::XsWindows::new().expect("Unable to initialize xenstore");

            std::XenstoreStd::new(xs)
                .run(channel)
                .await
                .expect("Xenstore failure")
        }
    }
}
