pub mod plugin;

use std::net::IpAddr;

use uuid::Uuid;

#[derive(Debug)]
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
    pub uuid: Uuid,
    pub index: u32,
    pub name: String,
    pub toolstack_iface: ToolstackNetInterface,
}

#[derive(Debug)]
pub enum NetEventOp {
    AddMac(String),
    RmMac(String),
    AddIp(IpAddr),
    RmIp(IpAddr),
}

#[derive(Debug)]
pub struct NetEvent {
    pub iface_id: Uuid,
    pub op: NetEventOp,
}

#[derive(Debug)]
pub struct OsInfo {
    pub os_info: os_info::Info,
    pub kernel_info: Option<KernelInfo>,
}

#[derive(Debug)]
pub struct MemoryInfo {
    pub mem_free: usize,
    pub mem_total: usize,
}

pub enum GuestMetric {
    OperatingSystem(OsInfo),
    AddIface(NetInterface),
    RmIface(Uuid),
    Memory(MemoryInfo),
    Network(NetEvent),
    CleanupIfaces,
}

pub use os_info;