use crate::datastructs::{NetEvent, NetEventOp, NetInterface, NetInterfaceCache};
use async_stream::try_stream;
use futures::channel::mpsc::UnboundedReceiver;
use futures::stream::{Stream, StreamExt};
use netlink_packet_core::{
    NetlinkHeader, NetlinkMessage, NetlinkPayload, NLM_F_DUMP, NLM_F_REQUEST,
};
use netlink_packet_route::{
    address, address::AddressMessage, link, link::LinkMessage, RouteNetlinkMessage,
};
use netlink_proto::{
    self, new_connection,
    sys::{protocols::NETLINK_ROUTE, AsyncSocket, SocketAddr},
};
use rtnetlink::constants::{RTMGRP_IPV4_IFADDR, RTMGRP_IPV6_IFADDR, RTMGRP_LINK};
use std::cell::RefCell;
use std::collections::hash_map;
use std::io;
use std::net::IpAddr;
use std::rc::Rc;
use std::vec::Vec;

use super::NetworkSource;

pub struct NetlinkNetworkSource {
    handle: netlink_proto::ConnectionHandle<RouteNetlinkMessage>,
    messages: UnboundedReceiver<(NetlinkMessage<RouteNetlinkMessage>, SocketAddr)>,
    iface_cache: NetInterfaceCache,
}

impl NetworkSource for NetlinkNetworkSource {
    fn new() -> io::Result<Self> {
        let (mut connection, handle, messages) = new_connection(NETLINK_ROUTE)?;
        // What kinds of broadcast messages we want to listen for.
        let nl_mgroup_flags = RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR;
        let nl_addr = SocketAddr::new(0, nl_mgroup_flags);
        connection
            .socket_mut()
            .socket_mut()
            .bind(&nl_addr)
            .expect("failed to bind");
        tokio::spawn(connection);
        Ok(NetlinkNetworkSource {
            handle,
            messages,
            iface_cache: Default::default(),
        })
    }

    async fn collect_current(&mut self) -> anyhow::Result<Vec<NetEvent>> {
        let mut events = Vec::<NetEvent>::new();

        // Create the netlink message that requests the links to be dumped
        let mut nl_hdr = NetlinkHeader::default();
        nl_hdr.flags = NLM_F_DUMP | NLM_F_REQUEST;
        let nl_msg = NetlinkMessage::new(
            nl_hdr,
            RouteNetlinkMessage::GetLink(LinkMessage::default()).into(),
        );
        // Send the request
        let mut nl_response = self.handle.request(nl_msg, SocketAddr::new(0, 0))?;
        // Handle response
        while let Some(packet) = nl_response.next().await {
            if let NetlinkMessage {
                payload: NetlinkPayload::InnerMessage(msg),
                ..
            } = packet
            {
                events.extend(self.netevent_from_rtnetlink(&msg)?);
            }
        }

        // Create the netlink message that requests the addresses to be dumped
        let mut nl_hdr = NetlinkHeader::default();
        nl_hdr.flags = NLM_F_DUMP | NLM_F_REQUEST;
        let nl_msg = NetlinkMessage::new(
            nl_hdr,
            RouteNetlinkMessage::GetAddress(AddressMessage::default()).into(),
        );
        // Send the request
        let mut nl_response = self.handle.request(nl_msg, SocketAddr::new(0, 0))?;
        // Handle response
        while let Some(packet) = nl_response.next().await {
            if let NetlinkMessage {
                payload: NetlinkPayload::InnerMessage(msg),
                ..
            } = packet
            {
                events.extend(self.netevent_from_rtnetlink(&msg)?);
            }
        }

        Ok(events)
    }

    fn stream(&mut self) -> impl Stream<Item = io::Result<NetEvent>> + '_ {
        try_stream! {
            while let Some((message, _)) = self.messages.next().await {
                if let NetlinkMessage{payload: NetlinkPayload::InnerMessage(msg), ..} = message {
                    for event in self.netevent_from_rtnetlink(&msg)? {
                        yield event;
                    }
                }
            };
        }
    }
}

impl NetlinkNetworkSource {
    fn netevent_from_rtnetlink(
        &mut self,
        nl_msg: &RouteNetlinkMessage,
    ) -> io::Result<Vec<NetEvent>> {
        let mut events = Vec::<NetEvent>::new();
        match nl_msg {
            RouteNetlinkMessage::NewLink(link_msg) => {
                let (iface, mac_address) = self.nl_linkmessage_decode(link_msg)?;
                log::debug!("NewLink({iface:?} {mac_address:?})");
                events.push(NetEvent {
                    iface: iface.clone(),
                    op: NetEventOp::AddIface,
                });
                if let Some(mac_address) = mac_address {
                    events.push(NetEvent {
                        iface,
                        op: NetEventOp::AddMac(mac_address),
                    });
                }
            }
            RouteNetlinkMessage::DelLink(link_msg) => {
                let (iface, mac_address) = self.nl_linkmessage_decode(link_msg)?;
                log::debug!("DelLink({iface:?} {mac_address:?})");
                if let Some(mac_address) = mac_address {
                    events.push(NetEvent {
                        iface: iface.clone(),
                        op: NetEventOp::RmMac(mac_address),
                    }); // redundant
                }
                events.push(NetEvent {
                    iface,
                    op: NetEventOp::RmIface,
                });
            }
            RouteNetlinkMessage::NewAddress(address_msg) => {
                // FIXME does not distinguish when IP is on DOWN iface
                let (iface, address) = self.nl_addressmessage_decode(address_msg)?;
                log::debug!("NewAddress({iface:?} {address})");
                events.push(NetEvent {
                    iface,
                    op: NetEventOp::AddIp(address),
                });
            }
            RouteNetlinkMessage::DelAddress(address_msg) => {
                let (iface, address) = self.nl_addressmessage_decode(address_msg)?;
                log::debug!("DelAddress({iface:?} {address})");
                events.push(NetEvent {
                    iface,
                    op: NetEventOp::RmIp(address),
                });
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unhandled RouteNetlinkMessage: {nl_msg:?}"),
                ));
            }
        };
        Ok(events)
    }

    fn nl_linkmessage_decode(
        &mut self,
        msg: &LinkMessage,
    ) -> io::Result<(
        Rc<RefCell<NetInterface>>, // ref to the (possibly new) impacted interface
        Option<String>,            // MAC address
    )> {
        let LinkMessage {
            header, attributes, ..
        } = msg;

        // extract fields of interest
        let mut iface_name: Option<String> = None;
        let mut address_bytes: Option<&Vec<u8>> = None;
        for nla in attributes {
            if let link::LinkAttribute::IfName(name) = nla {
                iface_name = Some(name.to_string());
            }
            if let link::LinkAttribute::Address(addr) = nla {
                address_bytes = Some(addr);
            }
        }
        // make sure message contains an address
        let mac_address = address_bytes.map(|address_bytes| {
            address_bytes
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<String>>()
                .join(":")
        });

        let iface = self
            .iface_cache
            .entry(header.index)
            .or_insert_with_key(|index| {
                RefCell::new(NetInterface::new(*index, iface_name.clone())).into()
            });

        // handle renaming
        if let Some(iface_name) = iface_name {
            let iface_renamed = iface.borrow().name != iface_name;
            if iface_renamed {
                log::trace!("name change: {iface:?} now named '{iface_name}'");
                iface.borrow_mut().name = iface_name;
            }
        };

        Ok((iface.clone(), mac_address))
    }

    fn nl_addressmessage_decode(
        &mut self,
        msg: &AddressMessage,
    ) -> io::Result<(Rc<RefCell<NetInterface>>, IpAddr)> {
        let AddressMessage {
            header, attributes, ..
        } = msg;

        // extract fields of interest
        let mut address: Option<&IpAddr> = None;
        for nla in attributes {
            if let address::AddressAttribute::Address(addr) = nla {
                address = Some(addr);
                break;
            }
        }

        let iface = match self.iface_cache.entry(header.index) {
            hash_map::Entry::Occupied(entry) => entry.get().clone(),
            hash_map::Entry::Vacant(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown interface for index {}", header.index),
                ));
            }
        };

        match address {
            Some(address) => Ok((iface.clone(), *address)),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown address",
            )),
        }
    }
}
