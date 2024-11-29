use guest_metrics::ToolstackNetInterface;

#[cfg(target_os = "freebsd")]
pub mod freebsd;
#[cfg(target_os = "linux")]
pub mod linux;

pub trait VifDetector: Default {
    fn get_toolstack_interface(iface_name: &str) -> Option<ToolstackNetInterface>;
}

#[cfg(target_os = "linux")]
pub type PlatformVifDetector = linux::LinuxVifDetector;

#[cfg(target_os = "freebsd")]
pub type PlatformVifDetector = freebsd::FreebsdVifDetector;
