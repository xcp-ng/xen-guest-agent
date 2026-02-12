use std::{collections::HashMap, sync::Arc, time::Duration};

use futures::{FutureExt, StreamExt};
use guest_metrics::{
    plugin::{GuestAgentPlugin, Shared},
    vif::VifDetector,
    GuestMetric, NetEvent, NetEventOp, NetInterface,
};
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use uuid::Uuid;

#[derive(Default)]
pub struct SimpleNetworkPlugin {
    interfaces: HashMap<String, NetworkInterface>,
    uuid_map: HashMap<String, Uuid>,
}

impl GuestAgentPlugin for SimpleNetworkPlugin {
    async fn run(mut self, shared: Arc<Shared>, channel: flume::Sender<GuestMetric>) {
        let mut timer = smol::Timer::interval(Duration::from_secs_f32(5.0));
        let vif_detector = &shared.vif_detector;
        let mut live_migration_stream = shared.live_migration_stream();

        loop {
            self.track_interfaces(vif_detector, &channel).await;

            futures::select! {
                _ = timer.next().fuse() => (),
                // Wipe cached state on live migration (we may resend the same info)
                _ = live_migration_stream.next().fuse() => self.flush()
            };
        }
    }
}

impl SimpleNetworkPlugin {
    pub fn flush(&mut self) {
        self.interfaces.clear();
        self.uuid_map.clear();
    }

    async fn track_interfaces(
        &mut self,
        vif_detector: &impl VifDetector,
        channel: &flume::Sender<GuestMetric>,
    ) {
        let interfaces = network_interface::NetworkInterface::show().unwrap();

        // Check for new interfaces (ones not in uuid_map)
        let new_interfaces = interfaces
            .iter()
            .filter(|interface| !self.uuid_map.contains_key(&interface.name))
            .collect::<Vec<_>>();

        let removed_interfaces = self
            .uuid_map
            .keys()
            .filter(|&name| !interfaces.iter().any(|interface| interface.name == *name))
            .cloned()
            .collect::<Vec<_>>();

        let changed_interfaces = interfaces
            .iter()
            .filter(|&interface| {
                if let Some(current) = self.interfaces.get(&interface.name) {
                    interface != current
                } else {
                    false
                }
            })
            .collect::<Vec<_>>();

        for interface in new_interfaces {
            let uuid = Uuid::new_v4();
            self.interfaces
                .insert(interface.name.clone(), interface.clone());
            self.uuid_map.insert(interface.name.clone(), uuid);

            channel
                .send_async(GuestMetric::AddIface(NetInterface {
                    uuid,
                    index: interface.index,
                    name: interface.name.clone(),
                    toolstack_iface: vif_detector
                        .get_toolstack_interface(&interface.name, interface.mac_addr.as_deref())
                        .await
                        .unwrap_or_default(),
                }))
                .await
                .unwrap();

            for addr in &interface.addr {
                channel
                    .send_async(GuestMetric::Network(NetEvent {
                        iface_id: uuid,
                        op: NetEventOp::AddIp(addr.ip()),
                    }))
                    .await
                    .unwrap();
            }

            if let Some(mac) = interface.mac_addr.clone() {
                channel
                    .send_async(GuestMetric::Network(NetEvent {
                        iface_id: uuid,
                        op: NetEventOp::AddMac(mac),
                    }))
                    .await
                    .unwrap();
            }
        }

        for interface in removed_interfaces {
            let uuid = self.uuid_map[&interface];

            channel
                .send_async(GuestMetric::RmIface(uuid))
                .await
                .unwrap();
            self.interfaces.remove(&interface);
            self.uuid_map.remove(&interface);
        }

        for interface in changed_interfaces {
            let current_interface = &self.interfaces[&interface.name];
            let uuid = self.uuid_map[&interface.name];

            // Check what is added and what is removed

            // Added addresses
            for addr in interface.addr.iter().filter(|&addr| {
                current_interface
                    .addr
                    .iter()
                    .all(|current_addr| addr != current_addr)
            }) {
                channel
                    .send_async(GuestMetric::Network(NetEvent {
                        iface_id: uuid,
                        op: NetEventOp::AddIp(addr.ip()),
                    }))
                    .await
                    .unwrap();
            }

            // Removed addresses
            for addr in current_interface.addr.iter().filter(|&addr| {
                interface
                    .addr
                    .iter()
                    .all(|current_addr| addr != current_addr)
            }) {
                channel
                    .send_async(GuestMetric::Network(NetEvent {
                        iface_id: uuid,
                        op: NetEventOp::RmIp(addr.ip()),
                    }))
                    .await
                    .unwrap();
            }

            // Changed MAC
            if interface.mac_addr != current_interface.mac_addr {
                if let Some(mac) = current_interface.mac_addr.clone() {
                    // Remove MAC
                    channel
                        .send_async(GuestMetric::Network(NetEvent {
                            iface_id: uuid,
                            op: NetEventOp::RmMac(mac),
                        }))
                        .await
                        .unwrap()
                }

                if let Some(mac) = interface.mac_addr.clone() {
                    // Remove MAC
                    channel
                        .send_async(GuestMetric::Network(NetEvent {
                            iface_id: uuid,
                            op: NetEventOp::AddMac(mac),
                        }))
                        .await
                        .unwrap()
                }
            }

            self.interfaces
                .insert(interface.name.clone(), interface.clone());
        }
    }
}
