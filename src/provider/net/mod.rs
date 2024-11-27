#[cfg(feature = "net_netlink")]
pub mod netlink;

#[cfg(feature = "net_pnet")]
pub mod pnet;

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use enum_dispatch::enum_dispatch;
use futures::{stream::Stream, StreamExt};

use crate::datastructs::NetEvent;

#[enum_dispatch]
pub trait NetworkSource: Sized + Stream<Item = Vec<NetEvent>> {
    async fn collect_current(&mut self) -> anyhow::Result<Vec<NetEvent>>;
}

pub struct DummyNetworkSource;

impl NetworkSource for DummyNetworkSource {
    async fn collect_current(&mut self) -> anyhow::Result<Vec<NetEvent>> {
        Ok(vec![])
    }
}

impl Stream for DummyNetworkSource {
    type Item = Vec<NetEvent>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

#[enum_dispatch(NetworkSource)]
pub enum AgentNetworkSource {
    Dummy(DummyNetworkSource),

    #[cfg(feature = "net_netlink")]
    Netlink(netlink::NetlinkNetworkSource),
    #[cfg(feature = "net_pnet")]
    Pnet(pnet::PnetNetworkSource),
}

// enum_dispatch doesn't support supertraits, we need to do that manually instead
impl Stream for AgentNetworkSource {
    type Item = Vec<NetEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
            AgentNetworkSource::Dummy(s) => s.poll_next_unpin(cx),
            #[cfg(feature = "net_netlink")]
            AgentNetworkSource::Netlink(s) => s.poll_next_unpin(cx),
            #[cfg(feature = "net_pnet")]
            AgentNetworkSource::Pnet(s) => s.poll_next_unpin(cx),
        }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum NetworkSourceKind {
    Dummy,
    #[cfg(feature = "net_netlink")]
    Netlink,
    #[cfg(feature = "net_pnet")]
    Pnet,
}

impl Default for NetworkSourceKind {
    fn default() -> Self {
        [
            #[cfg(feature = "net_netlink")]
            Self::Netlink,
            #[cfg(feature = "net_pnet")]
            Self::Pnet,
            Self::Dummy,
        ][0]
    }
}

impl AgentNetworkSource {
    pub fn new(kind: NetworkSourceKind) -> io::Result<Self> {
        match kind {
            NetworkSourceKind::Dummy => Ok(Self::Dummy(DummyNetworkSource)),
            #[cfg(feature = "net_netlink")]
            NetworkSourceKind::Netlink => Ok(Self::Netlink(netlink::NetlinkNetworkSource::new()?)),
            #[cfg(feature = "net_pnet")]
            NetworkSourceKind::Pnet => Ok(Self::Pnet(pnet::PnetNetworkSource::new()?)),
        }
    }
}
