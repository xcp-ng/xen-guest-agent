pub mod plugin;

use std::{fmt::Display, net::IpAddr};

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
pub enum OsVersion {
    Numbered(u64, u64, u64),
    Custom(String),
}

impl Display for OsVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsVersion::Numbered(a, b, c) => write!(f, "{a}.{b}.{c}"),
            OsVersion::Custom(version) => write!(f, "{version}"),
        }
    }
}

#[derive(Debug)]
pub struct OsBaseInfo {
    pub os_type: String,
    pub os_name: String,
    pub os_version: OsVersion,
}

#[derive(Debug)]
pub struct OsInfo {
    pub os_base_info: OsBaseInfo,
    pub kernel_info: Option<KernelInfo>,
}

#[derive(Debug)]
pub struct MemoryInfo {
    pub mem_free: usize,
    pub mem_total: usize,
}

pub type ClipboardData = Box<[u8]>;

pub enum GuestMetric {
    OperatingSystem(OsInfo),
    AddIface(NetInterface),
    RmIface(Uuid),
    Memory(MemoryInfo),
    Network(NetEvent),
    CleanupIfaces,
    /// clipboard data coming from the guest
    GetClipboard(ClipboardData),
}
