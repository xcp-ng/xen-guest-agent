use std::time::Duration;

use guest_metrics::OsBaseInfo;
use guest_metrics::{plugin::GuestAgentPlugin, GuestMetric, KernelInfo, OsInfo};
use std::io;

// UNIX uname() implementation
#[cfg(unix)]
fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    let uname_info = uname::uname()?;
    Ok(Some(KernelInfo {
        release: uname_info.release,
    }))
}

// default implementation
#[cfg(all(not(unix), not(windows)))]
fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    Ok(None)
}

fn collect_os() -> OsBaseInfo {
    let os_info = os_info::get();
    OsBaseInfo {
        os_type: os_info.os_type().to_string(),
        os_name: os_info.os_type().to_string(),
        os_version: os_info.version().to_string(),
    }
}

pub struct OsInfoPlugin;

async fn report_kernel(channel: &flume::Sender<GuestMetric>) {
    let kernel_info = collect_kernel().expect("Unable to fetch kernel information");

    channel
        .send_async(GuestMetric::OperatingSystem(OsInfo {
            os_base_info: collect_os(),
            kernel_info,
        }))
        .await
        .ok();
}

impl GuestAgentPlugin for OsInfoPlugin {
    async fn run(self, channel: flume::Sender<GuestMetric>) {
        let mut timer = smol::Timer::interval(Duration::from_secs_f32(60.0));

        loop {
            report_kernel(channel).await;
            timer.next().await;
        }
    }
}
