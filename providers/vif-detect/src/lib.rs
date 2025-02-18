use guest_metrics::ToolstackNetInterface;

#[cfg(target_os = "freebsd")]
pub mod freebsd;
#[cfg(target_os = "linux")]
pub mod linux;

pub trait VifDetector: Default {
    fn get_toolstack_interface(&self, iface_name: &str, mac_addr: Option<&str>) -> Option<ToolstackNetInterface>;
}

#[cfg(target_os = "linux")]
pub type PlatformVifDetector = linux::LinuxVifDetector;

#[cfg(target_os = "freebsd")]
pub type PlatformVifDetector = freebsd::FreebsdVifDetector;
