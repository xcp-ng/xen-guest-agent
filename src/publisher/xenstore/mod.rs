pub mod rfc;
pub mod std;

use ::std::io;

use xenstore_rs::Xs;

pub fn xs_publish(xs: &impl Xs, key: &str, value: &str) -> io::Result<()> {
    log::trace!("+ {}={:?}", key, value);
    xs.write(key, value)
}

pub fn xs_unpublish(xs: &impl Xs, key: &str) -> io::Result<()> {
    log::trace!("- {}", key);
    xs.rm(key)
}

pub trait XsBuild: Sized + Xs {
    fn new() -> io::Result<Self>;
}

#[cfg(target_family = "unix")]
impl XsBuild for xenstore_rs::unix::XsUnix {
    fn new() -> io::Result<Self> {
        Self::new()
    }
}

pub type PlatformXs = xenstore_rs::unix::XsUnix;
