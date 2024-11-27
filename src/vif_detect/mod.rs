use crate::datastructs::ToolstackNetInterface;

pub mod freebsd;
pub mod linux;

pub trait VifDetector: Default {
    fn get_toolstack_interface(iface_name: &str) -> Option<ToolstackNetInterface>;
}

#[derive(Default)]
struct DummyVifDetector;

impl VifDetector for DummyVifDetector {
    fn get_toolstack_interface(_iface_name: &str) -> Option<ToolstackNetInterface> {
        None
    }
}

#[cfg(target_os = "linux")]
pub type PlatformVifDetector = linux::LinuxVifDetector;

#[cfg(target_os = "freebsd")]
pub type PlatformVifDetector = freebsd::FreebsdVifDetector;
