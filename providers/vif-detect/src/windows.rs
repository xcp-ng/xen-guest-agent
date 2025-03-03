use std::sync::Mutex;

use guest_metrics::ToolstackNetInterface;
use xenstore_rs::Xs;
use xenstore_win::XsWindows;

pub struct WindowsVifDetector(Option<Mutex<XsWindows>>);

impl Default for WindowsVifDetector {
    fn default() -> Self {
        Self(
            XsWindows::new()
                .inspect_err(|e| log::warn!("Unable to load xenstore: {e}"))
                .ok()
                .map(Mutex::new),
        )
    }
}

impl super::VifDetector for WindowsVifDetector {
    fn get_toolstack_interface(
        &self,
        iface_name: &str,
        mac_addr: Option<&str>,
    ) -> Option<ToolstackNetInterface> {
        let xs = self.0.as_ref()?.lock().unwrap();
        let mac_addr = mac_addr?;

        log::info!("Probing {iface_name} (MAC: {mac_addr})");

        for vif_id in xs.directory("device/vif").ok()? {
            let Some(vif_mac) = xs.read(&format!("device/vif/{vif_id}/mac")).ok() else {
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
