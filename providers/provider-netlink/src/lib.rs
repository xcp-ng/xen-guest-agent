use std::{collections::HashMap, io, sync::Arc};

use guest_metrics::{
    plugin::{GuestAgentPlugin, Shared},
    vif::VifDetector,
    GuestMetric, NetEvent, NetEventOp, NetInterface,
};

use futures::{channel::mpsc::UnboundedReceiver, StreamExt};
use uuid::Uuid;

use netlink_packet_core::{
    NetlinkHeader, NetlinkMessage, NetlinkPayload, NLM_F_DUMP, NLM_F_REQUEST,
};
use netlink_packet_route::{
    address::{AddressAttribute, AddressMessage},
    link::{LinkAttribute, LinkMessage},
    RouteNetlinkMessage,
};
use netlink_proto::{
    new_connection,
    sys::{protocols::NETLINK_ROUTE, AsyncSocket, SocketAddr},
};
use rtnetlink::constants::{RTMGRP_IPV4_IFADDR, RTMGRP_IPV6_IFADDR, RTMGRP_LINK};

struct NetlinkConnection {
    handle: netlink_proto::ConnectionHandle<RouteNetlinkMessage>,
    messages: UnboundedReceiver<(NetlinkMessage<RouteNetlinkMessage>, SocketAddr)>,
}

impl NetlinkConnection {
    fn new() -> io::Result<Self> {
        let (mut connection, handle, messages) = new_connection(NETLINK_ROUTE)?;
        // What kinds of broadcast messages we want to listen for.
        let nl_mgroup_flags = RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV6_IFADDR;
        let nl_addr = SocketAddr::new(0, nl_mgroup_flags);
        connection
            .socket_mut()
            .socket_mut()
            .bind(&nl_addr)
            .expect("failed to bind to netlink");

        tokio::spawn(connection);
        Ok(Self { handle, messages })
    }
}

#[derive(Default)]
pub struct NetlinkPlugin;

impl GuestAgentPlugin for NetlinkPlugin {
    async fn run(self, shared: Arc<Shared>, channel: flume::Sender<GuestMetric>) {
        let connection = NetlinkConnection::new().unwrap();
        let vif_identify = &shared.vif_detector;
        let mut interfaces = HashMap::new();

        // Create the netlink message that requests the links to be dumped
        let mut nl_hdr = NetlinkHeader::default();
        nl_hdr.flags = NLM_F_DUMP | NLM_F_REQUEST;
        // Send the request
        let link_stream = connection
            .handle
            .request(
                NetlinkMessage::new(
                    nl_hdr,
                    RouteNetlinkMessage::GetLink(LinkMessage::default()).into(),
                ),
                SocketAddr::new(0, 0),
            )
            .unwrap();

        // Create the netlink message that requests the addresses to be dumped
        let mut nl_hdr = NetlinkHeader::default();
        nl_hdr.flags = NLM_F_DUMP | NLM_F_REQUEST;
        // Send the request
        let address_stream = connection
            .handle
            .request(
                NetlinkMessage::new(
                    nl_hdr,
                    RouteNetlinkMessage::GetAddress(AddressMessage::default()).into(),
                ),
                SocketAddr::new(0, 0),
            )
            .unwrap();

        let mut stream = link_stream
            .chain(address_stream)
            .chain(connection.messages.map(|(msg, _)| msg));

        while let Some(msg) = stream.next().await {
            if let NetlinkPayload::InnerMessage(inner_msg) = msg.payload {
                if let Err(e) =
                    process_message(inner_msg, &channel, vif_identify, &mut interfaces).await
                {
                    log::error!("Unable to process netlink message: {e}");
                }
            }
        }
    }
}

async fn process_message(
    inner_msg: RouteNetlinkMessage,
    channel: &flume::Sender<GuestMetric>,
    vif_identify: &impl VifDetector,
    interfaces: &mut HashMap<u32, Uuid>,
) -> anyhow::Result<()> {
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
                log::warn!("Ignoring NewLink/GetLink message without ifname");
                return Ok(());
            };

            let mac = link_message
                .attributes
                .iter()
                .find_map(|attribute| match attribute {
                    LinkAttribute::Address(addr) => Some(
                        addr.iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<Vec<String>>()
                            .join(":"),
                    ),
                    _ => None,
                });

            let Some(toolstack_iface) = vif_identify
                .get_toolstack_interface(ifname, mac.as_deref())
                .await
            else {
                log::debug!("Unknown interface {ifname} (mac: {mac:?})");
                return Ok(());
            };

            let uuid = Uuid::new_v4();

            interfaces.insert(link_message.header.index, uuid);
            channel
                .send_async(GuestMetric::AddIface(NetInterface {
                    uuid,
                    index: link_message.header.index,
                    name: ifname.clone(),
                    toolstack_iface,
                }))
                .await?;
        }
        RouteNetlinkMessage::DelLink(link_message) => {
            let Some(&uuid) = interfaces.get(&link_message.header.index) else {
                return Ok(());
            };

            channel.send_async(GuestMetric::RmIface(uuid)).await.ok();
        }
        RouteNetlinkMessage::NewAddress(address_message)
        | RouteNetlinkMessage::GetAddress(address_message) => {
            let Some(&iface_id) = interfaces.get(&address_message.header.index) else {
                log::warn!(
                    "Ignoring NewAddress/GetAddress on unknown interface with index={}",
                    address_message.header.index
                );
                return Ok(());
            };

            let Some(&addr) =
                address_message
                    .attributes
                    .iter()
                    .find_map(|attribute| match attribute {
                        AddressAttribute::Address(addr) => Some(addr),
                        _ => None,
                    })
            else {
                log::debug!("Got NewAddress/GetAddress without IP.");
                return Ok(());
            };

            channel
                .send_async(GuestMetric::Network(NetEvent {
                    iface_id,
                    op: NetEventOp::AddIp(addr),
                }))
                .await?;
        }
        RouteNetlinkMessage::DelAddress(address_message) => {
            let Some(&iface_id) = interfaces.get(&address_message.header.index) else {
                log::warn!(
                    "Ignoring DelAddress on unknown interface with index={}",
                    address_message.header.index
                );
                return Ok(());
            };

            let Some(&addr) =
                address_message
                    .attributes
                    .iter()
                    .find_map(|attribute| match attribute {
                        AddressAttribute::Address(addr) => Some(addr),
                        _ => None,
                    })
            else {
                log::debug!("Got DelAddress without IP.");
                return Ok(());
            };

            channel
                .send_async(GuestMetric::Network(NetEvent {
                    iface_id,
                    op: NetEventOp::RmIp(addr),
                }))
                .await?;
        }
        _ => {}
    }

    Ok(())
}
