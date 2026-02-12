use std::{io, sync::Arc};

use clap::ValueEnum;
#[cfg(target_os = "linux")]
use guest_metrics::vif::{self, PlatformVifDetector};
use guest_metrics::{
    plugin::{GuestAgentPlugin, Shared},
    GuestMetric,
};
use provider_simple::SimpleNetworkPlugin;

#[cfg(feature = "netlink")]
use provider_netlink::NetlinkPlugin;
use xenstore_rs::smol::XsSmol;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum NetworkPluginKind {
    Simple,
    #[cfg(feature = "netlink")]
    Netlink,
}

impl Default for NetworkPluginKind {
    fn default() -> Self {
        Self::Simple
    }
}

pub enum NetworkPlugin {
    Simple(SimpleNetworkPlugin),
    #[cfg(feature = "netlink")]
    Netlink(NetlinkPlugin),
}

impl NetworkPlugin {
    pub fn new(kind: NetworkPluginKind) -> io::Result<Self> {
        match kind {
            NetworkPluginKind::Simple => Ok(Self::Simple(SimpleNetworkPlugin::default())),
            #[cfg(feature = "netlink")]
            NetworkPluginKind::Netlink => Ok(Self::Netlink(NetlinkPlugin)),
        }
    }

    pub async fn run(self, shared: Arc<Shared>, channel: flume::Sender<GuestMetric>) {
        match self {
            NetworkPlugin::Simple(plugin) => plugin.run(shared, channel).await,
            #[cfg(feature = "netlink")]
            NetworkPlugin::Netlink(plugin) => plugin.run(shared, channel).await,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum VifDetectionMethod {
    #[cfg(target_os = "linux")]
    Linux,
    #[cfg(target_os = "freebsd")]
    Freebsd,
    Xenstore,
    None,
}

impl Default for VifDetectionMethod {
    fn default() -> Self {
        #[cfg(target_os = "linux")]
        return Self::Linux;

        #[cfg(target_os = "freebsd")]
        return Self::Freebsd;

        #[allow(unused)]
        Self::Xenstore
    }
}

pub fn build_platform_vif_detector(
    kind: VifDetectionMethod,
    xs: Option<XsSmol<'static>>,
) -> PlatformVifDetector {
    match kind {
        #[cfg(target_os = "linux")]
        VifDetectionMethod::Linux => {
            PlatformVifDetector::Linux(vif::linux::LinuxVifDetector::default())
        }
        #[cfg(target_os = "freebsd")]
        VifDetectionMethod::Freebsd => {
            PlatformVifDetector::Freebsd(vif::freebsd::FreebsdVifDetector::default())
        }
        VifDetectionMethod::Xenstore => {
            if let Some(xs) = xs {
                PlatformVifDetector::Xenstore(vif::xenstore::XenstoreVifDetector(xs))
            } else {
                log::error!("Can't use xenstore vif detection method without xenstore");
                PlatformVifDetector::None
            }
        }
        VifDetectionMethod::None => PlatformVifDetector::None,
    }
}
