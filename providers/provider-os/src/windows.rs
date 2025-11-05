use std::io;
use std::{collections::HashMap, error::Error, fmt::Display};

use guest_metrics::plugin::GuestAgentPlugin;
use guest_metrics::{GuestMetric, KernelInfo, OsBaseInfo, OsInfo, OsVersion};

use futures::StreamExt;
use xenstore_win::smol::XsSmolWindows;
use xenstore_win::suspend::AsyncSuspend;

use wmi::{COMLibrary, Variant, WMIConnection};
use xen_win_utils;

#[derive(Debug, Clone)]
struct CollectError {}

impl Error for CollectError {}

impl Display for CollectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "An error occurred when collecting OS information")
    }
}

fn os_caption() -> Result<String, Box<dyn Error>> {
    let com_lib = COMLibrary::new()?;
    let wmi = WMIConnection::new(com_lib)?;
    let captions: Vec<HashMap<String, Variant>> =
        wmi.raw_query("SELECT Caption FROM Win32_OperatingSystem")?;
    let caption = captions
        .iter()
        .map(|hm| {
            hm.get("Caption").and_then(|variant| {
                if let wmi::Variant::String(caption) = variant {
                    Some(caption)
                } else {
                    None
                }
            })
        })
        .find_map(|v| v)
        .ok_or(Box::new(CollectError {}))?;
    Ok(caption.clone())
}

fn collect_os() -> OsBaseInfo {
    let (major, minor, build) = xen_win_utils::get_version().unwrap_or((0, 0, 0));
    let version = OsVersion::Numbered(major.into(), minor.into(), build.into());
    OsBaseInfo {
        os_type: "Windows".to_string(),
        os_name: os_caption().unwrap_or(format!("Windows {}", version)),
        os_version: version,
    }
}

fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    let (major, minor, build) = xen_win_utils::get_version()?;
    Ok(Some(KernelInfo {
        release: format!("{0}.{1}.{2}", major, minor, build),
    }))
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
        let xs_smol = XsSmolWindows::new()
            .await
            .expect("Unable to start async xenstore");
        let mut suspend = xs_smol
            .register_suspend()
            .await
            .expect("Unable to register suspend callback");

        loop {
            report_kernel(&channel).await;
            suspend.next().await;
        }
    }
}
