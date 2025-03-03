use std::io;

use guest_metrics::{plugin::GuestAgentPlugin, GuestMetric};
use provider_simple::SimpleNetworkPlugin;

#[cfg(feature = "netlink")]
use provider_netlink::NetlinkPlugin;

#[derive(Clone, Copy, Debug, argh::FromArgValue)]
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

    pub async fn run(self, channel: flume::Sender<GuestMetric>) {
        match self {
            NetworkPlugin::Simple(plugin) => plugin.run(channel).await,
            #[cfg(feature = "netlink")]
            NetworkPlugin::Netlink(plugin) => plugin.run(channel).await,
        }
    }
}
