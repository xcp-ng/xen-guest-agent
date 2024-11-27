use crate::datastructs::{NetEvent, NetEventOp};
use crate::publisher::{MemoryInfo, OsInfo, Publisher};
use std::io;
use std::net::IpAddr;
use xenstore_rs::Xs;

use super::{xs_publish, xs_unpublish};

#[derive(Clone)]
pub struct XenstoreRfc<XS: Xs>(XS);

const PROTOCOL_VERSION: &str = env!("CARGO_PKG_VERSION");

// FIXME: this should be a runtime config of xenstore-std.rs

impl<XS: Xs + 'static> XenstoreRfc<XS> {
    pub fn new(xs: XS) -> Self {
        XenstoreRfc(xs)
    }
}

impl<XS: Xs> Publisher for XenstoreRfc<XS> {
    fn publish_osinfo(&mut self, info: &OsInfo) -> io::Result<()> {
        xs_publish(&self.0, "data/xen-guest-agent", PROTOCOL_VERSION)?;
        xs_publish(
            &self.0,
            "data/os/name",
            &format!("{} {}", info.os_info.os_type(), info.os_info.version()),
        )?;
        xs_publish(
            &self.0,
            "data/os/version",
            &info.os_info.version().to_string(),
        )?;
        xs_publish(&self.0, "data/os/class", "unix")?;
        if let Some(kernel_info) = &info.kernel_info {
            xs_publish(&self.0, "data/os/unix/kernel-version", &kernel_info.release)?;
        }

        Ok(())
    }

    fn cleanup_ifaces(&mut self) -> io::Result<()> {
        // Currently only vif interfaces are cleaned
        xs_unpublish(&self.0, "data/net")
    }

    fn publish_memory(&mut self, _mem_info: &MemoryInfo) -> io::Result<()> {
        //xs_publish(&self.xs, "data/meminfo_free", &mem_free_kb.to_string())?;
        Ok(())
    }

    #[allow(clippy::useless_format)]
    fn publish_netevent(&mut self, event: &NetEvent) -> io::Result<()> {
        let iface_id = &event.iface.lock().unwrap().index;
        let xs_iface_prefix = format!("data/net/{iface_id}");
        match &event.op {
            NetEventOp::AddIface => {
                xs_publish(
                    &self.0,
                    &format!("{xs_iface_prefix}"),
                    &event.iface.lock().unwrap().name,
                )?;
            }
            NetEventOp::RmIface => {
                xs_unpublish(&self.0, &format!("{xs_iface_prefix}"))?;
            }
            NetEventOp::AddIp(address) => {
                let key_suffix = munged_address(address);
                xs_publish(&self.0, &format!("{xs_iface_prefix}/{key_suffix}"), "")?;
            }
            NetEventOp::RmIp(address) => {
                let key_suffix = munged_address(address);
                xs_unpublish(&self.0, &format!("{xs_iface_prefix}/{key_suffix}"))?;
            }
            NetEventOp::AddMac(mac_address) => {
                xs_publish(&self.0, &format!("{xs_iface_prefix}"), mac_address)?;
            }
            NetEventOp::RmMac(_) => {
                xs_unpublish(&self.0, &format!("{xs_iface_prefix}"))?;
            }
        }
        Ok(())
    }
}

fn munged_address(addr: &IpAddr) -> String {
    match addr {
        IpAddr::V4(addr) => "ipv4/".to_string() + &addr.to_string().replace('.', "_"),
        IpAddr::V6(addr) => "ipv6/".to_string() + &addr.to_string().replace(':', "_"),
    }
}
