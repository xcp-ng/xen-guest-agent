use crate::datastructs::{NetEvent, NetEventOp, NetInterface, ToolstackNetInterface};
use crate::publisher::Publisher;
use crate::publisher::{MemoryInfo, OsInfo};
use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use xenstore_rs::Xs;

use super::{xs_publish, xs_unpublish};

pub struct XenstoreStd<XS: Xs + 'static> {
    xs: XS,
    // use of integer indices for IP addresses requires to keep a mapping
    ip_addresses: IpList,

    // control/feature-balloon is a control node of XAPI's squeezed,
    // and gets created by the guest because xenopsd sets ~/control/
    // with domain ownership.  OTOH libxl creates it readonly, so we
    // catch the case where it is so to avoid uselessly retrying.
    forbidden_control_feature_balloon: bool,
}

const NUM_IFACE_IPS: usize = 10;
type IfaceIpList = [Option<IpAddr>; NUM_IFACE_IPS];
struct IfaceIpStruct {
    v4: IfaceIpList,
    v6: IfaceIpList,
}
type IpList = HashMap<u32, IfaceIpStruct>;

// pseudo version for xe-daemon compatibility, real agent version in
// BuildVersion below
const AGENT_VERSION_MAJOR: &str = "1"; // XO does not show version at all if 0
const AGENT_VERSION_MINOR: &str = "0";
const AGENT_VERSION_MICRO: &str = "0"; // XAPI exposes "-1" if missing

impl<XS: Xs + 'static> XenstoreStd<XS> {
    pub fn new(xs: XS) -> Self {
        let ip_addresses = IpList::new();
        XenstoreStd {
            xs,
            ip_addresses,
            forbidden_control_feature_balloon: false,
        }
    }
}

impl<XS: Xs> Publisher for XenstoreStd<XS> {
    fn publish_osinfo(&mut self, info: &OsInfo) -> io::Result<()> {
        // FIXME this is not anywhere standard, just minimal XS compatibility
        xs_publish(&self.xs, "attr/PVAddons/MajorVersion", AGENT_VERSION_MAJOR)?;
        xs_publish(&self.xs, "attr/PVAddons/MinorVersion", AGENT_VERSION_MINOR)?;
        xs_publish(&self.xs, "attr/PVAddons/MicroVersion", AGENT_VERSION_MICRO)?;
        let agent_version_build = format!("proto-{}", &env!("CARGO_PKG_VERSION"));
        xs_publish(&self.xs, "attr/PVAddons/BuildVersion", &agent_version_build)?;

        xs_publish(
            &self.xs,
            "data/os_distro",
            &info.os_info.os_type().to_string(),
        )?;
        xs_publish(
            &self.xs,
            "data/os_name",
            &format!("{} {}", info.os_info.os_type(), info.os_info.version()),
        )?;
        // FIXME .version only has "major" component right now; not a
        // big deal for a proto, os_minorver is known to be unreliable
        // in xe-guest-utilities at least for Debian
        let os_version = info.os_info.version();
        match os_version {
            os_info::Version::Semantic(major, minor, _patch) => {
                xs_publish(&self.xs, "data/os_majorver", &major.to_string())?;
                xs_publish(&self.xs, "data/os_minorver", &minor.to_string())?;
            }
            _ => {
                // FIXME what to do with strings?
                // the lack of `os_*ver` is anyway not a big deal
                log::info!("cannot parse yet os version {:?}", os_version);
            }
        }
        if let Some(kernel_info) = &info.kernel_info {
            xs_publish(&self.xs, "data/os_uname", &kernel_info.release)?;
        }

        if !self.forbidden_control_feature_balloon {
            // we may want to be more clever some day, e.g. by
            // checking if the guest indeed has ballooning, and if the
            // balloon driver has reached the requested initial
            // `~/memory/target` value (or, possibly, to rely on the
            // balloon driver to do the job of signaling this
            // condition)
            match xs_publish(&self.xs, "control/feature-balloon", "1") {
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    log::warn!("cannot write control/feature-balloon (impacts XAPI's squeezed)");
                    self.forbidden_control_feature_balloon = true;
                }
                Ok(_) => (),
                e => return e,
            }
        }

        Ok(())
    }

    fn publish_memory(&mut self, mem_info: &MemoryInfo) -> io::Result<()> {
        xs_publish(
            &self.xs,
            "data/meminfo_free",
            &(mem_info.mem_free / 1024).to_string(),
        )?;
        xs_publish(
            &self.xs,
            "data/meminfo_total",
            &(mem_info.mem_total / 1024).to_string(),
        )?;

        Ok(())
    }

    // see https://xenbits.xen.org/docs/unstable/misc/xenstore-paths.html#domain-controlled-paths
    fn publish_netevent(&mut self, event: &NetEvent) -> io::Result<()> {
        let iface_id = match event.iface.borrow().toolstack_iface {
            ToolstackNetInterface::Vif(id) => id,
        };
        let xs_iface_prefix = format!("attr/vif/{iface_id}");
        match &event.op {
            NetEventOp::AddIface => {
                xs_publish(&self.xs, &xs_iface_prefix, "")?;
            }
            NetEventOp::RmIface => {
                xs_unpublish(&self.xs, &xs_iface_prefix)?;
            }
            NetEventOp::AddIp(address) => {
                let key_suffix = self.munged_address(address, &event.iface.borrow())?;
                xs_publish(
                    &self.xs,
                    &format!("{xs_iface_prefix}/{key_suffix}"),
                    &address.to_string(),
                )?;
            }
            NetEventOp::RmIp(address) => {
                let key_suffix = self.munged_address(address, &event.iface.borrow())?;
                xs_unpublish(&self.xs, &format!("{xs_iface_prefix}/{key_suffix}"))?;
            }

            // FIXME extend IfaceIpStruct for this
            NetEventOp::AddMac(_mac_address) => {
                log::debug!("AddMac not applied");
            }
            NetEventOp::RmMac(_mac_address) => {
                log::debug!("RmMac not applied");
            }
        }
        Ok(())
    }

    fn cleanup_ifaces(&mut self) -> io::Result<()> {
        // Currently only vif interfaces are cleaned
        xs_unpublish(&self.xs, "attr/vif")
    }
}

impl<XS: Xs> XenstoreStd<XS> {
    fn munged_address(&mut self, addr: &IpAddr, iface: &NetInterface) -> io::Result<String> {
        let ip_entry = self
            .ip_addresses
            .entry(iface.index)
            .or_insert(IfaceIpStruct {
                v4: [None; NUM_IFACE_IPS],
                v6: [None; NUM_IFACE_IPS],
            });
        let ip_list = match addr {
            IpAddr::V4(_) => &mut ip_entry.v4,
            IpAddr::V6(_) => &mut ip_entry.v6,
        };
        let ip_slot = get_ip_slot(addr, ip_list)?;
        match addr {
            IpAddr::V4(_) => Ok(format!("ipv4/{ip_slot}")),
            IpAddr::V6(_) => Ok(format!("ipv6/{ip_slot}")),
        }
    }
}

fn get_ip_slot(ip: &IpAddr, list: &mut IfaceIpList) -> io::Result<usize> {
    let mut empty_idx: Option<usize> = None;
    for (idx, item) in list.iter().enumerate() {
        match item {
            Some(item) => {
                if item == ip {
                    return Ok(idx);
                }
            } // found
            None => {
                if empty_idx.is_none() {
                    empty_idx = Some(idx)
                }
            }
        }
    }
    // not found, insert in empty space if possible
    if let Some(idx) = empty_idx {
        list[idx] = Some(*ip);
        return Ok(idx);
    }
    Err(io::Error::new(
        io::ErrorKind::OutOfMemory, /*StorageFull?*/
        "no free slot for a new IP address",
    ))
}
