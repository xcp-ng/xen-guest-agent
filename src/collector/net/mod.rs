pub mod netlink;

#[cfg(feature = "pnet")]
pub mod pnet;

use crate::datastructs::NetEvent;
use futures::stream::Stream;
use std::error::Error;
use std::io;

pub trait NetworkSource: Sized {
    fn new() -> io::Result<Self>;
    async fn collect_current(&mut self) -> Result<Vec<NetEvent>, Box<dyn Error>>;
    fn stream(&mut self) -> impl Stream<Item = io::Result<NetEvent>> + '_;
}

pub struct DummyNetworkSource;

impl NetworkSource for DummyNetworkSource {
    fn new() -> io::Result<Self> {
        Ok(Self)
    }

    async fn collect_current(&mut self) -> Result<Vec<NetEvent>, Box<dyn Error>> {
        Ok(vec![])
    }

    fn stream(&mut self) -> impl Stream<Item = io::Result<NetEvent>> + '_ {
        futures::stream::empty::<io::Result<NetEvent>>()
    }
}

pub type PlatformNetworkSource = netlink::NetlinkNetworkSource;
