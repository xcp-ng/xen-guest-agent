use crate::datastructs::ToolstackNetInterface;

#[derive(Default)]
pub struct FreebsdVifDetector;

impl super::VifDetector for FreebsdVifDetector {
    // identifies a VIF as named "xn%ID"
    fn get_toolstack_interface(iface_name: &str) -> Option<ToolstackNetInterface> {
        const PREFIX: &str = "xn";
        if !iface_name.starts_with(PREFIX) {
            log::debug!("ignoring interface {iface_name} as not starting with '{PREFIX}'");
            return None;
        }

        let index = iface_name[PREFIX.len()..]
            .parse()
            .inspect_err(|e| log::error!("cannot parse a VIF number adter {PREFIX}: {e}"))
            .ok()?;

        Some(ToolstackNetInterface::Vif(index))
    }
}
