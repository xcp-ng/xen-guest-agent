use std::fs;

use guest_metrics::ToolstackNetInterface;

use super::VifDetector;

// identifies a VIF from sysfs as devtype="vif", and take the VIF id
// from nodename="device/vif/$ID"

// FIXME does not attempt to detect sr-iov VIFs

#[derive(Default)]
pub struct LinuxVifDetector;

impl VifDetector for LinuxVifDetector {
    fn get_toolstack_interface(&self, iface_name: &str, _mac_addr: Option<&str>) -> Option<ToolstackNetInterface> {
        // FIXME: using ETHTOOL ioctl could be better
        let device_path = format!("/sys/class/net/{iface_name}/device");
        let devtype = fs::read_to_string(format!("{device_path}/devtype"))
            .inspect_err(|e| log::debug!("reading {device_path}/devtype: {e}"))
            .ok()?;

        let "vif" = devtype.trim() else {
            log::debug!("ignoring device {device_path}, devtype {devtype:?} not 'vif'");
            return None;
        };

        let nodename = fs::read_to_string(format!("{device_path}/nodename"))
            .inspect_err(|e| log::error!("reading {device_path}/nodename: {e}"))
            .ok()?;
        let nodename = nodename.trim();

        const PREFIX: &str = "device/vif/";
        if !nodename.starts_with(PREFIX) {
            log::debug!("ignoring interface {nodename} as not under {PREFIX}");
            return None;
        }
        let vif_id = nodename[PREFIX.len()..].parse().unwrap();

        Some(ToolstackNetInterface::Vif(vif_id))
    }
}
