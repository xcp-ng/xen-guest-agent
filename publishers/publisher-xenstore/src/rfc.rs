use futures::{channel::mpsc, StreamExt};
use guest_metrics::{
    GuestMetric, MemoryInfo, NetEvent, NetEventOp, NetInterface, OsInfo, ToolstackNetInterface,
};
use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use uuid::Uuid;
use xenstore_rs::Xs;

use super::{xs_publish, xs_unpublish};

#[derive(Clone)]
pub struct XenstoreRfc<XS: Xs> {
    xs: XS,

    ifaces: HashMap<Uuid, NetInterface>,
}

const PROTOCOL_VERSION: &str = env!("CARGO_PKG_VERSION");

fn iface_prefix(iface_id: u32) -> String {
    format!("data/net/{iface_id}")
}

// FIXME: this should be a runtime config of xenstore-std.rs

impl<XS: Xs + 'static> XenstoreRfc<XS> {
    pub fn new(xs: XS) -> Self {
        XenstoreRfc {
            xs,
            ifaces: HashMap::new(),
        }
    }

    fn publish_osinfo(&mut self, info: &OsInfo) -> io::Result<()> {
        xs_publish(&self.xs, "data/xen-guest-agent", PROTOCOL_VERSION)?;
        xs_publish(
            &self.xs,
            "data/os/name",
            &format!("{} {}", info.os_info.os_type(), info.os_info.version()),
        )?;
        xs_publish(
            &self.xs,
            "data/os/version",
            &info.os_info.version().to_string(),
        )?;
        xs_publish(&self.xs, "data/os/class", "unix")?;
        if let Some(kernel_info) = &info.kernel_info {
            xs_publish(
                &self.xs,
                "data/os/unix/kernel-version",
                &kernel_info.release,
            )?;
        }

        Ok(())
    }

    fn cleanup_ifaces(&mut self) -> io::Result<()> {
        // Currently only vif interfaces are cleaned
        xs_unpublish(&self.xs, "data/net")
    }

    fn publish_memory(&mut self, _mem_info: &MemoryInfo) -> io::Result<()> {
        //xs_publish(&self.xs, "data/meminfo_free", &mem_free_kb.to_string())?;
        Ok(())
    }

    #[allow(clippy::useless_format)]
    fn publish_netevent(&mut self, event: &NetEvent) -> io::Result<()> {
        let Some(iface) = self.ifaces.get(&event.iface_id) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Got event from unknown interface ({})", event.iface_id),
            ));
        };

        let ToolstackNetInterface::Vif(iface_id) = iface.toolstack_iface else {
            log::warn!("Got event from unsupported interface {:?}", iface);
            return Ok(());
        };

        let xs_iface_prefix = iface_prefix(iface_id);
        match &event.op {
            NetEventOp::AddIp(address) => {
                let key_suffix = munged_address(address);
                xs_publish(&self.xs, &format!("{xs_iface_prefix}/{key_suffix}"), "")?;
            }
            NetEventOp::RmIp(address) => {
                let key_suffix = munged_address(address);
                xs_unpublish(&self.xs, &format!("{xs_iface_prefix}/{key_suffix}"))?;
            }
            NetEventOp::AddMac(mac_address) => {
                xs_publish(&self.xs, &format!("{xs_iface_prefix}"), mac_address)?
            }
            NetEventOp::RmMac(_) => xs_unpublish(&self.xs, &format!("{xs_iface_prefix}"))?,
        }
        Ok(())
    }

    pub async fn run(mut self, mut channel: mpsc::Receiver<GuestMetric>) -> io::Result<()> {
        while let Some(metric) = channel.next().await {
            match metric {
                GuestMetric::OperatingSystem(os_info) => self.publish_osinfo(&os_info)?,
                GuestMetric::Memory(memory_info) => self.publish_memory(&memory_info)?,
                GuestMetric::Network(net_event) => self.publish_netevent(&net_event)?,
                GuestMetric::CleanupIfaces => self.cleanup_ifaces()?,
                GuestMetric::AddIface(net_interface) => {
                    if let ToolstackNetInterface::Vif(iface_id) = net_interface.toolstack_iface {
                        xs_publish(&self.xs, &iface_prefix(iface_id), "")?;
                    }
                    self.ifaces.insert(net_interface.uuid, net_interface);
                }
                GuestMetric::RmIface(uuid) => {
                    if let Some(interface) = self.ifaces.remove(&uuid) {
                        if let ToolstackNetInterface::Vif(iface_id) = interface.toolstack_iface {
                            xs_unpublish(&self.xs, &iface_prefix(iface_id))?;
                        }
                    }
                }
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
