use std::io;

use crate::datastructs::KernelInfo;

// UNIX uname() implementation
#[cfg(unix)]
pub fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    let uname_info = uname::uname()?;
    Ok(Some(KernelInfo {
        release: uname_info.release,
    }))
}

// default implementation
#[cfg(not(unix))]
pub fn collect_kernel() -> io::Result<Option<KernelInfo>> {
    Ok(None)
}
