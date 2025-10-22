use std::{collections::HashMap, error::Error, fmt::Display};

use guest_metrics::{OsBaseInfo, OsVersion};

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

pub fn collect_os() -> OsBaseInfo {
    let (major, minor, build) = xen_win_utils::get_version().unwrap_or((0, 0, 0));
    let version = OsVersion::Numbered(major.into(), minor.into(), build.into());
    OsBaseInfo {
        os_type: "Windows".to_string(),
        os_name: os_caption().unwrap_or(format!("Windows {}", version)),
        os_version: version,
    }
}
