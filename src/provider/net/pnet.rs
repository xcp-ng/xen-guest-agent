use crate::datastructs::{NetEvent, NetEventOp, NetInterface, NetInterfaceCache};
use futures::ready;
use futures::stream::Stream;
use ipnetwork::IpNetwork;
use pnet_base::MacAddr;
use std::collections::{hash_map, HashMap, HashSet};
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::Interval;

use super::NetworkSource;

const IFACE_PERIOD_SECONDS: f32 = 60.0;

#[derive(Debug, Eq, Hash, PartialEq)]
enum Address {
    IP(IpNetwork),
    MAC(MacAddr),
}
struct InterfaceInfo {
    // only needed to keep iface name from pnet data until we know we
    // have a new NetInterface to construct
    name: String,
    addresses: HashSet<Address>,
}

impl InterfaceInfo {
    pub fn new(name: &str) -> InterfaceInfo {
        InterfaceInfo {
            name: name.to_string(),
            addresses: HashSet::new(),
        }
    }
}

type AddressesState = HashMap<u32, InterfaceInfo>;
pub struct PnetNetworkSource {
    addresses_cache: AddressesState,
    iface_cache: NetInterfaceCache,
    timer: Interval,
}

impl PnetNetworkSource {}

impl NetworkSource for PnetNetworkSource {
    async fn collect_current(&mut self) -> anyhow::Result<Vec<NetEvent>> {
        Ok(self.get_ifconfig_data()?)
    }
}

impl Stream for PnetNetworkSource {
    type Item = Vec<NetEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        ready!(self.timer.poll_tick(cx));

        Poll::Ready(
            self.get_ifconfig_data()
                .inspect_err(|e| log::error!("Unable to fetch network data {e}"))
                .ok(),
        )
    }
}

impl PnetNetworkSource {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            addresses_cache: AddressesState::new(),
            iface_cache: HashMap::new(),
            timer: tokio::time::interval(Duration::from_secs_f32(IFACE_PERIOD_SECONDS)),
        })
    }

    fn get_ifconfig_data(&mut self) -> io::Result<Vec<NetEvent>> {
        let network_interfaces = pnet_datalink::interfaces();

        // get a full view of interfaces, diffable with addresses_cache
        let mut current_addresses = AddressesState::new();
        for iface in network_interfaces.iter() {
            // KLUDGE: drop ":alias" suffix for Linux interface aliases
            let name = iface.name.split(":").next().unwrap_or(&iface.name);
            let entry = current_addresses
                .entry(iface.index)
                .or_insert_with(|| InterfaceInfo::new(name));
            for ip in &iface.ips {
                entry.addresses.insert(Address::IP(*ip));
            }
            if let Some(mac) = iface.mac {
                entry.addresses.insert(Address::MAC(mac));
            }
        }

        // diff addresses_cache and current_addresses view

        // events to be returned
        let mut events = vec![];
        // pseudo-const to get a valid reference for unwrap_or
        let empty_address_set: HashSet<Address> = HashSet::new();

        // disappearing addresses
        for (cached_iface_index, cached_info) in self.addresses_cache.iter() {
            // iface object from iface_cache
            let iface = match self.iface_cache.entry(*cached_iface_index) {
                hash_map::Entry::Occupied(entry) => entry.get().clone(),
                hash_map::Entry::Vacant(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "disappearing interface with index {} not in iface_cache",
                            cached_iface_index
                        ),
                    ));
                }
            };
            // notify addresses or full iface removal
            match current_addresses.get(cached_iface_index) {
                Some(iface_info) => {
                    let iface_adresses = &iface_info.addresses;
                    for disappearing in cached_info.addresses.difference(iface_adresses) {
                        log::trace!(
                            "disappearing {}: {:?}",
                            iface.lock().unwrap().name,
                            disappearing
                        );
                        events.push(NetEvent {
                            iface: iface.clone(),
                            op: match disappearing {
                                Address::IP(ip) => NetEventOp::RmIp(ip.ip()),
                                Address::MAC(mac) => NetEventOp::RmMac((*mac).to_string()),
                            },
                        });
                    }
                }
                None => {
                    events.push(NetEvent {
                        iface: iface.clone(),
                        op: NetEventOp::RmIface,
                    });
                }
            };
        }

        // appearing addresses
        for (iface_index, iface_info) in current_addresses.iter() {
            let iface = self
                .iface_cache
                .entry(*iface_index)
                .or_insert_with_key(|index| {
                    let iface = Arc::new(Mutex::new(NetInterface::new(
                        *index,
                        Some(iface_info.name.clone()),
                    )));
                    events.push(NetEvent {
                        iface: iface.clone(),
                        op: NetEventOp::AddIface,
                    });
                    iface
                })
                .clone();
            let cache_adresses = if let Some(cache_info) = self.addresses_cache.get(iface_index) {
                &cache_info.addresses
            } else {
                &empty_address_set
            };
            for appearing in iface_info.addresses.difference(cache_adresses) {
                log::trace!("appearing {}: {:?}", iface.lock().unwrap().name, appearing);
                events.push(NetEvent {
                    iface: iface.clone(),
                    op: match appearing {
                        Address::IP(ip) => NetEventOp::AddIp(ip.ip()),
                        Address::MAC(mac) => NetEventOp::AddMac((*mac).to_string()),
                    },
                });
            }
        }

        // replace cache with view
        self.addresses_cache = current_addresses; // FIXME expensive?

        Ok(events)
    }
}
