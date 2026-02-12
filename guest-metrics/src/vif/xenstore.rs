use crate::ToolstackNetInterface;
use xenstore_rs::{smol::XsSmol, AsyncXs};

/// Vif detector that matches mac address with xenstore info.
pub struct XenstoreVifDetector(pub XsSmol<'static>);

impl super::VifDetector for XenstoreVifDetector {
    async fn get_toolstack_interface(
        &self,
        iface_name: &str,
        mac_addr: Option<&str>,
    ) -> Option<ToolstackNetInterface> {
        let mac_addr = mac_addr?;

        log::info!("Probing {iface_name} (MAC: {mac_addr})");

        for vif_id in self.0.directory("device/vif").await.ok()? {
            let Some(vif_mac) = self.0.read(&format!("device/vif/{vif_id}/mac")).await.ok() else {
                log::warn!("vif/{vif_id} has no MAC address");
                continue;
            };

            if mac_addr.trim().eq_ignore_ascii_case(vif_mac.trim()) {
                log::info!("{iface_name} is vif/{vif_id}");
                return Some(ToolstackNetInterface::Vif(
                    vif_id.parse().expect("Unable to parse vif id"),
                ));
            }
        }

        Some(ToolstackNetInterface::Unknown)
    }
}
