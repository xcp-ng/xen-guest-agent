// default no-op Publisher implementation
pub mod xenstore;

use crate::datastructs::{KernelInfo, NetEvent, NetEventOp};
use enum_dispatch::enum_dispatch;
use os_info;
use std::{env, io};
use xenstore::{rfc::XenstoreRfc, std::XenstoreStd};
use xenstore_rs::Xs;

#[enum_dispatch]
pub trait Publisher: Sized {
    fn publish_static(
        &mut self,
        os_info: &os_info::Info,
        kernel_info: &Option<KernelInfo>,
        mem_total_kb: Option<usize>,
    ) -> io::Result<()>;
    fn publish_memfree(&mut self, mem_free_kb: usize) -> io::Result<()>;
    fn publish_netevent(&mut self, event: &NetEvent) -> io::Result<()>;

    fn cleanup_ifaces(&mut self) -> io::Result<()>;
}

#[derive(Default)]
pub struct ConsolePublisher;

impl Publisher for ConsolePublisher {
    fn publish_static(
        &mut self,
        os_info: &os_info::Info,
        kernel_info: &Option<KernelInfo>,
        mem_total_kb: Option<usize>,
    ) -> io::Result<()> {
        println!("OS: {} - Version: {}", os_info.os_type(), os_info.version());
        if let Some(mem_total_kb) = mem_total_kb {
            println!("Total memory: {mem_total_kb} KB");
        }
        if let Some(KernelInfo { release }) = kernel_info {
            println!("Kernel version: {}", release);
        }
        Ok(())
    }
    fn publish_memfree(&mut self, mem_free_kb: usize) -> io::Result<()> {
        println!("Free memory: {mem_free_kb} KB");
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

impl<XS: Xs> AgentPublisher<XS> {
    pub fn new(xs: XS) -> io::Result<Self> {
        match env::var("XENSTORE_PUBLISHER").unwrap_or_default().as_str() {
            "console" => Ok(Self::Console(ConsolePublisher::default())),
            "rfc" => Ok(Self::XenstoreRfc(XenstoreRfc::new(xs))),
            "std" | _ => Ok(Self::XenstoreStd(XenstoreStd::new(xs))),
        }
    }
}
