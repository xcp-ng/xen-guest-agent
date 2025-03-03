use std::collections::HashMap;

use guest_metrics::{
    plugin::GuestAgentPublisher, GuestMetric, KernelInfo, NetEventOp, NetInterface,
    ToolstackNetInterface,
};
use uuid::Uuid;

#[derive(Default)]
pub struct ConsolePublisher {
    ifaces: HashMap<Uuid, NetInterface>,
}

impl ConsolePublisher {
    fn process_message(&mut self, metric: GuestMetric) {
        match metric {
            GuestMetric::OperatingSystem(os_info) => {
                println!(
                    "OS: {} - Version: {}",
                    os_info.os_info.os_type(),
                    os_info.os_info.version()
                );
                if let Some(KernelInfo { release }) = &os_info.kernel_info {
                    println!("Kernel version: {release}");
                }
            }
            GuestMetric::Memory(memory_info) => {
                println!(
                    "Memory: {}/{} KB",
                    memory_info.mem_free / 1024,
                    memory_info.mem_total / 1024
                );
            }
            GuestMetric::AddIface(iface) => {
                let ifkind = match iface.toolstack_iface {
                    ToolstackNetInterface::Unknown => String::from("unknown"),
                    ToolstackNetInterface::Vif(id) => format!("vif/{id}"),
                    _ => todo!(),
                };
                println!("{} +IFACE ({})", iface.index, ifkind);
                self.ifaces.insert(iface.uuid, iface);
            }
            GuestMetric::RmIface(iface_id) => {
                let Some((_, iface)) = self.ifaces.remove_entry(&iface_id) else {
                    return;
                };

                println!("{} -IFACE", iface.index);
            }
            GuestMetric::Network(net_event) => {
                let Some(iface) = self.ifaces.get(&net_event.iface_id) else {
                    return;
                };

                match &net_event.op {
                    NetEventOp::AddIp(address) => println!("{} +IP  {address}", iface.index),
                    NetEventOp::RmIp(address) => println!("{} -IP  {address}", iface.index),
                    NetEventOp::AddMac(mac_address) => {
                        println!("{} +MAC {mac_address}", iface.index)
                    }
                    NetEventOp::RmMac(mac_address) => {
                        println!("{} -MAC {mac_address}", iface.index)
                    }
                }
            }
            GuestMetric::CleanupIfaces => {}
        }
    }
}

impl GuestAgentPublisher for ConsolePublisher {
    async fn run(mut self, channel: flume::Receiver<GuestMetric>) {
        while let Ok(msg) = channel.recv_async().await {
            self.process_message(msg)
        }
    }
}
