use std::{collections::HashMap, io};

use futures::{channel::mpsc::UnboundedReceiver, StreamExt};
use guest_metrics::plugin::GuestAgentPlugin;
use netlink_packet_core::{NetlinkMessage, NetlinkPayload};
use netlink_packet_route::{link::LinkAttribute, RouteNetlinkMessage};
use netlink_proto::{
    new_connection,
    sys::{protocols::NETLINK_ROUTE, AsyncSocket, SocketAddr},
};
use rtnetlink::constants::{RTMGRP_IPV4_IFADDR, RTMGRP_IPV6_IFADDR, RTMGRP_LINK};
use uuid::Uuid;

pub struct NetlinkConnection {
    handle: netlink_proto::ConnectionHandle<RouteNetlinkMessage>,
    messages: UnboundedReceiver<(NetlinkMessage<RouteNetlinkMessage>, SocketAddr)>,

    interfaces: HashMap<String, Uuid>,
}

impl NetlinkConnection {
    pub fn new() -> io::Result<Self> {
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
        Ok(Self { handle, messages })
    }
}

pub struct NetlinkPlugin;

impl GuestAgentPlugin for NetlinkPlugin {
    fn run(
        self,
        mut channel: futures::channel::mpsc::Sender<guest_metrics::GuestMetric>,
    ) -> impl std::future::Future<Output = ()> + Send {
        async move {
            let mut connection = NetlinkConnection::new().unwrap();

            if let Some((msg, _)) = connection.messages.next().await {
                if let NetlinkPayload::InnerMessage(inner_msg) = msg.payload {
                    process_message(inner_msg);
                }
            }
        }
    }
}

fn process_message(inner_msg: RouteNetlinkMessage) {
    match inner_msg {
        RouteNetlinkMessage::NewLink(link_message) | RouteNetlinkMessage::GetLink(link_message) => {
            let Some(ifname) =
                link_message
                    .attributes
                    .iter()
                    .find_map(|attribute| match attribute {
                        LinkAttribute::IfName(n) => Some(n),
                        _ => None,
                    })
            else {
                return;
            };

            let Some(mac) = link_message.attributes.iter().find_map(|attribute| match attribute {
                LinkAttribute::Address()
            })
        }
        RouteNetlinkMessage::DelLink(link_message) => {
            todo!()
        }
        RouteNetlinkMessage::NewAddress(address_message) => {
            todo!()
        }
        RouteNetlinkMessage::DelAddress(address_message) => {
            todo!()
        }
        RouteNetlinkMessage::GetAddress(address_message) => {
            todo!()
        }
        _ => todo!(),
    }
}
