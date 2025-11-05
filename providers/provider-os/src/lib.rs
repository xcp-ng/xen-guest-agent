#[cfg(not(windows))]
pub mod unix;
#[cfg(not(windows))]
pub use crate::unix::OsInfoPlugin;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use crate::windows::OsInfoPlugin;
