use std::cell::RefCell;
use std::collections::HashMap;
use std::net::IpAddr;
use std::rc::Rc;

use crate::vif_detect::{PlatformVifDetector, VifDetector};

pub struct KernelInfo {
    pub release: String,
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum ToolstackNetInterface {
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
            toolstack_iface: PlatformVifDetector::get_toolstack_interface(&name).unwrap(),
        }
    }
}

// The cache of currently-known network interfaces.  We have to use
// reference counting on the cached items, as we want on one hand to
// use references to those items from NetEvent, and OTOH we want to
// remove interfaces from here once unplugged.  And Rust won't let us
// use `&'static NetInterface` because we can do the latter, which is
// good in the end.
// The interface may change name after creation (hence `RefCell`).
pub type NetInterfaceCache = HashMap<u32, Rc<RefCell<NetInterface>>>;

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
    pub iface: Rc<RefCell<NetInterface>>,
    pub op: NetEventOp,
}
