use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;

use crate::vif_detect::{PlatformVifDetector, VifDetector};

pub struct KernelInfo {
    pub release: String,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default)]
pub enum ToolstackNetInterface {
    #[default]
    Unknown,
    Vif(u32),
    // SRIOV,
    // PciPassthrough,
    // UsbPassthrough,
}

#[derive(Clone, Debug)]
pub struct NetInterface {
    pub index: u32,
    pub name: String,
    pub toolstack_iface: ToolstackNetInterface,
}

impl NetInterface {
    pub fn new(index: u32, name: Option<String>) -> NetInterface {
        let name = match name {
            Some(string) => string,
            None => {
                log::error!("new interface with index {index} has no name");
                String::from("") // this is not valid, but user will now be aware
            }
        };
        NetInterface {
            index,
            name: name.clone(),
            toolstack_iface: PlatformVifDetector::get_toolstack_interface(&name)
                .unwrap_or_default(),
        }
    }
}

// TODO: Teddy: We should find a better solution than abusing Arc<Mutex>>

// The cache of currently-known network interfaces.  We have to use
// reference counting on the cached items, as we want on one hand to
// use references to those items from NetEvent, and OTOH we want to
// remove interfaces from here once unplugged.  And Rust won't let us
// use `&'static NetInterface` because we can do the latter, which is
// good in the end.
// The interface may change name after creation (hence `RefCell`).
pub type NetInterfaceCache = HashMap<u32, Arc<Mutex<NetInterface>>>;

#[derive(Debug)]
pub enum NetEventOp {
    AddIface,
    RmIface,
    AddMac(String),
    RmMac(String),
    AddIp(IpAddr),
    RmIp(IpAddr),
}

#[derive(Debug)]
pub struct NetEvent {
    pub iface: Arc<Mutex<NetInterface>>,
    pub op: NetEventOp,
}
