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
