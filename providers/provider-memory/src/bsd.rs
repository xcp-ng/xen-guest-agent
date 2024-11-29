use std::io;
use sysctl::Sysctl;

use super::MemorySource;

pub struct BsdMemorySource {
    memtotal_ctl: sysctl::Ctl,
    pagesize: usize,
    meminactive_ctl: sysctl::Ctl,
    memcache_ctl: sysctl::Ctl,
    memfree_ctl: sysctl::Ctl,
}

impl MemorySource for BsdMemorySource {
    fn new() -> io::Result<Self> {
        let pagesize_ctl = new_sysctl("hw.pagesize")?;
        let pagesize_value = pagesize_ctl.value().map_err(sysctrlerror_to_ioerror)?;
        let pagesize = *pagesize_value.as_int().ok_or(io::Error::new(
            io::ErrorKind::InvalidData,
            "cannot parse hw.pagesize as int",
        ))? as usize;
        Ok(Self {
            memtotal_ctl: new_sysctl("hw.physmem")?,
            pagesize,
            meminactive_ctl: new_sysctl("vm.stats.vm.v_inactive_count")?,
            memcache_ctl: new_sysctl("vm.stats.vm.v_cache_count")?,
            memfree_ctl: new_sysctl("vm.stats.vm.v_free_count")?,
        })
    }
    fn get_total_kb(&mut self) -> io::Result<usize> {
        Ok(get_field_ulong(&self.memtotal_ctl)? as usize / 1024)
    }
    fn get_available_kb(&mut self) -> io::Result<usize> {
        let available = (get_field_u32(&self.meminactive_ctl)? as usize
            + get_field_uint(&self.memcache_ctl)? as usize
            + get_field_u32(&self.memfree_ctl)? as usize)
            * self.pagesize;
        Ok(available / 1024)
    }
}

// helper to create a sysctl with errors mapped to io::Error
fn new_sysctl(name: &str) -> io::Result<sysctl::Ctl> {
    match sysctl::Ctl::new(name) {
        Err(sysctl::SysctlError::NotFound(_)) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("sysctl {} not found", name),
        )),
        Err(e) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Unexpected error type creating sysctl::Ctl: {:?}", e),
        )),
        Ok(ctl) => Ok(ctl),
    }
}

fn sysctrlerror_to_ioerror(error: sysctl::SysctlError) -> io::Error {
    match error {
        e => io::Error::new(io::ErrorKind::Other, format!("sysctl error: {:?}", e)),
    }
}

fn get_field_ulong(ctl: &sysctl::Ctl) -> io::Result<u64> {
    let v = ctl.value().map_err(sysctrlerror_to_ioerror)?;
    if let Some(value) = v.as_ulong() {
        Ok(*value)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "cannot interpret {} as ulong: {:?}",
                ctl.name().map_err(sysctrlerror_to_ioerror)?,
                v
            ),
        ))
    }
}

fn get_field_u32(ctl: &sysctl::Ctl) -> io::Result<u32> {
    let v = ctl.value().map_err(sysctrlerror_to_ioerror)?;
    if let Some(value) = v.as_u32() {
        Ok(*value)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "cannot interpret {} as u32: {:?}",
                ctl.name().map_err(sysctrlerror_to_ioerror)?,
                v
            ),
        ))
    }
}

fn get_field_uint(ctl: &sysctl::Ctl) -> io::Result<u32> {
    let v = ctl.value().map_err(sysctrlerror_to_ioerror)?;
    if let Some(value) = v.as_uint() {
        Ok(*value)
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "cannot interpret {} as uint: {:?}",
                ctl.name().map_err(sysctrlerror_to_ioerror)?,
                v
            ),
        ))
    }
}
