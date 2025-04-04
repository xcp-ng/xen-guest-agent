use guest_metrics::{os_info, plugin::GuestAgentPlugin, GuestMetric, KernelInfo, OsInfo};
use std::io;

#[cfg(windows)]
use xen_win_utils;

// UNIX uname() implementation
#[cfg(unix)]
pub fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    let uname_info = uname::uname()?;
    Ok(Some(KernelInfo {
        release: uname_info.release,
    }))
}

#[cfg(windows)]
pub fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    let version_info = xen_win_utils::get_version()?;
    Ok(Some(guest_metrics::KernelInfo {
        release: version_info,
    }))
}

// default implementation
#[cfg(all(not(unix), not(windows)))]
pub fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    Ok(None)
}

pub struct OsInfoPlugin;

impl GuestAgentPlugin for OsInfoPlugin {
    async fn run(self, channel: flume::Sender<guest_metrics::GuestMetric>) {
        let kernel_info = collect_kernel().expect("Unable to fetch kernel information");

        channel
            .send_async(GuestMetric::OperatingSystem(OsInfo {
                os_info: os_info::get(),
                kernel_info,
            }))
            .await
            .ok();
    }
}
