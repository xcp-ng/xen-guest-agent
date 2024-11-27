pub mod netlink;

#[cfg(feature = "pnet")]
pub mod pnet;

use crate::datastructs::NetEvent;
use futures::stream::Stream;
use std::io;

pub trait NetworkSource: Sized {
    fn new() -> io::Result<Self>;
    async fn collect_current(&mut self) -> anyhow::Result<Vec<NetEvent>>;
    fn stream(&mut self) -> impl Stream<Item = io::Result<NetEvent>> + '_;
}

pub struct DummyNetworkSource;

impl NetworkSource for DummyNetworkSource {
    fn new() -> io::Result<Self> {
        Ok(Self)
    }

    async fn collect_current(&mut self) -> anyhow::Result<Vec<NetEvent>> {
        Ok(vec![])
    }

    fn stream(&mut self) -> impl Stream<Item = io::Result<NetEvent>> + '_ {
        futures::stream::empty::<io::Result<NetEvent>>()
    }
}

pub type PlatformNetworkSource = netlink::NetlinkNetworkSource;
