use crate::datastructs::{KernelInfo, NetEvent, NetEventOp};
use crate::publisher::{xs_publish, xs_unpublish, XenstoreSchema};
use std::io;
use std::net::IpAddr;
use xenstore_rs::Xs;

pub struct Schema<XS: Xs>(XS);

const PROTOCOL_VERSION: &str = env!("CARGO_PKG_VERSION");

// FIXME: this should be a runtime config of xenstore-std.rs

impl<XS: Xs + 'static> Schema<XS> {
    pub fn new(xs: XS) -> Box<dyn XenstoreSchema> {
        Box::new(Schema(xs))
    }
}

impl<XS: Xs> XenstoreSchema for Schema<XS> {
    fn publish_static(&mut self, os_info: &os_info::Info, kernel_info: &Option<KernelInfo>,
                      _mem_total_kb: Option<usize>,
    ) -> io::Result<()> {
        xs_publish(&self.0, "data/xen-guest-agent", PROTOCOL_VERSION)?;
        xs_publish(&self.0, "data/os/name",
                   &format!("{} {}", os_info.os_type(), os_info.version()))?;
        xs_publish(&self.0, "data/os/version", &os_info.version().to_string())?;
        xs_publish(&self.0, "data/os/class", "unix")?;
        if let Some(kernel_info) = kernel_info {
            xs_publish(&self.0, "data/os/unix/kernel-version", &kernel_info.release)?;
        }

        Ok(())
    }

    fn cleanup_ifaces(&mut self) -> io::Result<()> {
        // Currently only vif interfaces are cleaned
        xs_unpublish(&self.0, "data/net")
    }

    fn publish_memfree(&self, _mem_free_kb: usize) -> io::Result<()> {
        //xs_publish(&self.xs, "data/meminfo_free", &mem_free_kb.to_string())?;
        Ok(())
    }

    #[allow(clippy::useless_format)]
    fn publish_netevent(&mut self, event: &NetEvent) -> io::Result<()> {
        let iface_id = &event.iface.borrow().index;
        let xs_iface_prefix = format!("data/net/{iface_id}");
        match &event.op {
            NetEventOp::AddIface => {
                xs_publish(&self.0, &format!("{xs_iface_prefix}"), &event.iface.borrow().name)?;
            },
            NetEventOp::RmIface => {
                xs_unpublish(&self.0, &format!("{xs_iface_prefix}"))?;
            },
            NetEventOp::AddIp(address) => {
                let key_suffix = munged_address(address);
                xs_publish(&self.0, &format!("{xs_iface_prefix}/{key_suffix}"), "")?;
            },
            NetEventOp::RmIp(address) => {
                let key_suffix = munged_address(address);
                xs_unpublish(&self.0, &format!("{xs_iface_prefix}/{key_suffix}"))?;
            },
            NetEventOp::AddMac(mac_address) => {
                xs_publish(&self.0, &format!("{xs_iface_prefix}"), mac_address)?;
            },
            NetEventOp::RmMac(_) => {
                xs_unpublish(&self.0, &format!("{xs_iface_prefix}"))?;
            },
        }
        Ok(())
    }
}

fn munged_address(addr: &IpAddr) -> String {
    match addr {
        IpAddr::V4(addr) =>
            "ipv4/".to_string() + &addr.to_string().replace('.', "_"),
        IpAddr::V6(addr) =>
            "ipv6/".to_string() + &addr.to_string().replace(':', "_"),
    }
}
