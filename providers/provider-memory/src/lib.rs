use std::{io, time::Duration};

use futures::{channel::mpsc, SinkExt};
use guest_metrics::{plugin::GuestAgentPlugin, MemoryInfo};

#[cfg(target_os = "freebsd")]
pub mod bsd;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

pub trait MemorySource: Sized {
    fn new() -> io::Result<Self>;
    fn get_total_kb(&mut self) -> io::Result<usize>;
    fn get_available_kb(&mut self) -> io::Result<usize>;
}

#[cfg(target_os = "linux")]
pub type PlatformMemorySource = linux::LinuxMemorySource;

#[cfg(target_os = "freebsd")]
pub type PlatformMemorySource = bsd::BsdMemorySource;

#[cfg(target_os = "windows")]
pub type PlatformMemorySource = windows::WindowsMemorySource;

pub struct MemoryPlugin;

impl GuestAgentPlugin for MemoryPlugin {
    fn run(
        self,
        mut channel: mpsc::Sender<guest_metrics::GuestMetric>,
    ) -> impl std::future::Future<Output = ()> + Send {
        async move {
            let mut timer = tokio::time::interval(Duration::from_secs_f32(5.0));
            let mut memory_source =
                PlatformMemorySource::new().expect("Unable to get memory information");

            loop {
                timer.tick().await;

                if channel
                    .send(guest_metrics::GuestMetric::Memory(MemoryInfo {
                        mem_free: memory_source.get_available_kb().unwrap(),
                        mem_total: memory_source.get_total_kb().unwrap(),
                    }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}
