use guest_metrics::{
    GuestMetric, MemoryInfo, NetEvent, NetEventOp, NetInterface, OsInfo, ToolstackNetInterface,
};
use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use uuid::Uuid;
use xenstore_rs::smol::XsSmol;

use super::{xs_publish, xs_unpublish};

#[derive(Clone)]
pub struct XenstoreRfc {
    xs: XsSmol<'static>,
    ifaces: HashMap<Uuid, NetInterface>,
}

fn iface_prefix(iface_id: u32) -> String {
    format!("data/net/{iface_id}")
}

// FIXME: this should be a runtime config of xenstore-std.rs

impl XenstoreRfc {
    pub fn new(xs: XsSmol<'static>) -> Self {
        XenstoreRfc {
            xs,
            ifaces: HashMap::new(),
        }
    }

    async fn publish_agent_info(&mut self) -> io::Result<()> {
        xs_publish(&self.xs, "data/xen-guest-agent", env!("CARGO_PKG_VERSION")).await?;
        if let Some(vendor) = option_env!("GUEST_AGENT_VENDOR") {
            xs_publish(&self.xs, "data/xen-guest-agent/vendor", vendor).await?;
        }
        let major = env!("CARGO_PKG_VERSION_MAJOR");
        xs_publish(&self.xs, "data/xen-guest-agent/major", major).await?;
        let minor = env!("CARGO_PKG_VERSION_MINOR");
        xs_publish(&self.xs, "data/xen-guest-agent/minor", minor).await?;
        let patch = env!("CARGO_PKG_VERSION_PATCH");
        xs_publish(&self.xs, "data/xen-guest-agent/patch", patch).await?;

        let version_pre = env!("CARGO_PKG_VERSION_PRE");
        let channel = if version_pre.is_empty() {
            "stable"
        } else {
            version_pre
        };
        xs_publish(&self.xs, "data/xen-guest-agent/channel", channel).await?;

        Ok(())
    }

    async fn publish_osinfo(&mut self, info: &OsInfo) -> io::Result<()> {
        self.publish_agent_info().await?;

        xs_publish(
            &self.xs,
            "data/os/name",
            &format!("{} {}", info.os_info.os_type(), info.os_info.version()),
        )
        .await?;
        xs_publish(
            &self.xs,
            "data/os/version",
            &info.os_info.version().to_string(),
        )
        .await?;
        xs_publish(&self.xs, "data/os/class", "unix").await?;
        if let Some(kernel_info) = &info.kernel_info {
            xs_publish(
                &self.xs,
                "data/os/unix/kernel-version",
                &kernel_info.release,
            )
            .await?;
        }

        Ok(())
    }

    async fn cleanup_ifaces(&mut self) -> io::Result<()> {
        // Currently only vif interfaces are cleaned
        xs_unpublish(&self.xs, "data/net").await
    }

    async fn publish_memory(&mut self, _mem_info: &MemoryInfo) -> io::Result<()> {
        //xs_publish(&self.xs, "data/meminfo_free", &mem_free_kb.to_string())?;
        Ok(())
    }

    #[allow(clippy::useless_format)]
    async fn publish_netevent(&mut self, event: &NetEvent) -> io::Result<()> {
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
                xs_publish(&self.xs, &format!("{xs_iface_prefix}/{key_suffix}"), "").await?;
            }
            NetEventOp::RmIp(address) => {
                let key_suffix = munged_address(address);
                xs_unpublish(&self.xs, &format!("{xs_iface_prefix}/{key_suffix}")).await?;
            }
            NetEventOp::AddMac(mac_address) => {
                xs_publish(&self.xs, &format!("{xs_iface_prefix}"), mac_address).await?
            }
            NetEventOp::RmMac(_) => xs_unpublish(&self.xs, &format!("{xs_iface_prefix}")).await?,
        }
        Ok(())
    }

    pub async fn run(mut self, channel: flume::Receiver<GuestMetric>) -> io::Result<()> {
        while let Ok(metric) = channel.recv_async().await {
            match metric {
                GuestMetric::OperatingSystem(os_info) => self.publish_osinfo(&os_info).await?,
                GuestMetric::Memory(memory_info) => self.publish_memory(&memory_info).await?,
                GuestMetric::Network(net_event) => self.publish_netevent(&net_event).await?,
                GuestMetric::CleanupIfaces => self.cleanup_ifaces().await?,
                GuestMetric::AddIface(net_interface) => {
                    if let ToolstackNetInterface::Vif(iface_id) = net_interface.toolstack_iface {
                        xs_publish(&self.xs, &iface_prefix(iface_id), "").await?;
                    }
                    self.ifaces.insert(net_interface.uuid, net_interface);
                }
                GuestMetric::RmIface(uuid) => {
                    if let Some(interface) = self.ifaces.remove(&uuid) {
                        if let ToolstackNetInterface::Vif(iface_id) = interface.toolstack_iface {
                            xs_unpublish(&self.xs, &iface_prefix(iface_id)).await?;
                        }
                    }
                }
                GuestMetric::GetClipboard(_) => {
                    // TODO
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
