use std::io;

#[cfg(target_os = "freebsd")]
pub mod bsd;

#[cfg(target_os = "linux")]
pub mod linux;

pub trait MemorySource: Sized {
    fn new() -> io::Result<Self>;
    fn get_total_kb(&mut self) -> io::Result<usize>;
    fn get_available_kb(&mut self) -> io::Result<usize>;
}

#[derive(Default)]
pub struct DummyMemorySource;

impl MemorySource for DummyMemorySource {
    fn new() -> io::Result<Self> {
        Ok(Self)
    }

    fn get_total_kb(&mut self) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no implementation for mem_total",
        ))
    }
    fn get_available_kb(&mut self) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no implementation for mem_avail",
        ))
    }
}

#[cfg(target_os = "linux")]
pub type PlatformMemorySource = linux::LinuxMemorySource;

#[cfg(target_os = "freebsd")]
pub type PlatformMemorySource = bsd::BsdMemorySource;
