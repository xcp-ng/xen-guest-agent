use std::io;

use futures::channel::mpsc;
use guest_metrics::{plugin::GuestAgentPlugin, GuestMetric};
use provider_simple::SimpleNetworkPlugin;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum NetworkPluginKind {
    Simple,
}

impl Default for NetworkPluginKind {
    fn default() -> Self {
        Self::Simple
    }
}

pub enum NetworkPlugin {
    Simple(SimpleNetworkPlugin),
}

impl NetworkPlugin {
    pub fn new(kind: NetworkPluginKind) -> io::Result<Self> {
        match kind {
            NetworkPluginKind::Simple => Ok(Self::Simple(SimpleNetworkPlugin::default())),
        }
    }

    pub async fn run(self, channel: mpsc::Sender<GuestMetric>) {
        match self {
            NetworkPlugin::Simple(plugin) => plugin.run(channel).await,
        }
    }
}
