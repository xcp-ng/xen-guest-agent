// default no-op Publisher implementation
pub mod xenstore;

use crate::datastructs::{KernelInfo, NetEvent, NetEventOp};
use enum_dispatch::enum_dispatch;
use std::{env, io};
use xenstore::{rfc::XenstoreRfc, std::XenstoreStd, XsBuild};
use xenstore_rs::Xs;

pub struct OsInfo {
    pub os_info: os_info::Info,
    pub kernel_info: Option<KernelInfo>,
}

pub struct MemoryInfo {
    pub mem_free: usize,
    pub mem_total: usize,
}

#[enum_dispatch]
pub trait Publisher: Sized {
    fn publish_osinfo(&mut self, os_info: &OsInfo) -> io::Result<()>;
    fn publish_memory(&mut self, mem_info: &MemoryInfo) -> io::Result<()>;
    fn publish_netevent(&mut self, event: &NetEvent) -> io::Result<()>;

    fn cleanup_ifaces(&mut self) -> io::Result<()>;
}

#[derive(Default)]
pub struct ConsolePublisher;

impl Publisher for ConsolePublisher {
    fn publish_osinfo(&mut self, os_info: &OsInfo) -> io::Result<()> {
        println!(
            "OS: {} - Version: {}",
            os_info.os_info.os_type(),
            os_info.os_info.version()
        );
        if let Some(KernelInfo { release }) = &os_info.kernel_info {
            println!("Kernel version: {release}");
        }
        Ok(())
    }
    fn publish_memory(&mut self, mem_info: &MemoryInfo) -> io::Result<()> {
        println!(
            "Memory: {}/{} KB",
            mem_info.mem_free / 1024,
            mem_info.mem_total / 1024
        );
        Ok(())
    }
    fn publish_netevent(&mut self, event: &NetEvent) -> io::Result<()> {
        let iface_id = &event.iface.borrow().name;
        match &event.op {
            NetEventOp::AddIface => println!("{iface_id} +IFACE"),
            NetEventOp::RmIface => println!("{iface_id} -IFACE"),
            NetEventOp::AddIp(address) => println!("{iface_id} +IP  {address}"),
            NetEventOp::RmIp(address) => println!("{iface_id} -IP  {address}"),
            NetEventOp::AddMac(mac_address) => println!("{iface_id} +MAC {mac_address}"),
            NetEventOp::RmMac(mac_address) => println!("{iface_id} -MAC {mac_address}"),
        }
        Ok(())
    }

    fn cleanup_ifaces(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[enum_dispatch(Publisher)]
pub enum AgentPublisher<XS: Xs + 'static> {
    Console(ConsolePublisher),
    XenstoreRfc(XenstoreRfc<XS>),
    XenstoreStd(XenstoreStd<XS>),
}

impl<XS: XsBuild> AgentPublisher<XS> {
    #[allow(clippy::wildcard_in_or_patterns)]
    pub fn new() -> io::Result<Self> {
        match env::var("XENSTORE_PUBLISHER").unwrap_or_default().as_str() {
            "console" => Ok(Self::Console(ConsolePublisher)),
            "rfc" => Ok(Self::XenstoreRfc(XenstoreRfc::new(XS::new()?))),
            "std" | _ => Ok(Self::XenstoreStd(XenstoreStd::new(XS::new()?))),
        }
    }
}
