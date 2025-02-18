use std::io;

use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::MemorySource;

#[derive(Default)]
pub struct WindowsMemorySource;

fn read_memstatus_ex() -> io::Result<MEMORYSTATUSEX> {
    let mut mem_status = MEMORYSTATUSEX::default();
    mem_status.dwLength = size_of_val(&mem_status) as u32;

    unsafe { GlobalMemoryStatusEx(&mut mem_status).map_err(io::Error::other)? };
    Ok(mem_status)
}

impl MemorySource for WindowsMemorySource {
    fn new() -> io::Result<Self> {
        Ok(Self)
    }

    fn get_total_kb(&mut self) -> io::Result<usize> {
        Ok(read_memstatus_ex()?.ullTotalPhys as usize)
    }

    fn get_available_kb(&mut self) -> io::Result<usize> {
        Ok(read_memstatus_ex()?.ullAvailPhys as usize)
    }
}
