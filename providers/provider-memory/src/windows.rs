#[derive(Default)]
pub struct WindowsMemorySource;

impl MemorySource for WindowsMemorySource {
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
