/// Logic to identify a guest Vif.
use std::future::Future;

use crate::ToolstackNetInterface;

#[cfg(target_os = "freebsd")]
pub mod freebsd;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod xenstore;

pub trait VifDetector {
    fn get_toolstack_interface(
        &self,
        iface_name: &str,
        mac_addr: Option<&str>,
    ) -> impl Future<Output = Option<ToolstackNetInterface>> + Send;
}

pub enum PlatformVifDetector {
    #[cfg(target_os = "linux")]
    Linux(linux::LinuxVifDetector),
    #[cfg(target_os = "freebsd")]
    Freebsd(freebsd::FreebsdVifDetector),
    Xenstore(xenstore::XenstoreVifDetector),
    None,
}

impl VifDetector for PlatformVifDetector {
    async fn get_toolstack_interface(
        &self,
        iface_name: &str,
        mac_addr: Option<&str>,
    ) -> Option<ToolstackNetInterface> {
        match self {
            #[cfg(target_os = "linux")]
            PlatformVifDetector::Linux(linux_vif_detector) => {
                linux_vif_detector
                    .get_toolstack_interface(iface_name, mac_addr)
                    .await
            }
            #[cfg(target_os = "freebsd")]
            PlatformVifDetector::Freebsd(freebsd_vif_detector) => {
                freebsd_vif_detector
                    .get_toolstack_interface(iface_name, mac_addr)
                    .await
            }
            PlatformVifDetector::Xenstore(xenstore_vif_detector) => {
                xenstore_vif_detector
                    .get_toolstack_interface(iface_name, mac_addr)
                    .await
            }
            PlatformVifDetector::None => None,
        }
    }
}
