use std::io;

use guest_metrics::{plugin::GuestAgentPublisher, GuestMetric};
use publisher_console::ConsolePublisher;
use publisher_xenstore::{XenstoreRfcPublisher, XenstoreStdPublisher};

#[derive(Clone, Copy, Default, Debug, clap::ValueEnum)]
pub enum PublisherKind {
    Console,
    #[default]
    Xenstore,
    XenstoreRfc,
}

pub enum AgentPublisher {
    Console(ConsolePublisher),
    XenstoreRfc(XenstoreRfcPublisher),
    XenstoreStd(XenstoreStdPublisher),
}

impl AgentPublisher {
    pub fn new(kind: PublisherKind) -> io::Result<Self> {
        match kind {
            PublisherKind::Console => Ok(Self::Console(ConsolePublisher::default())),
            PublisherKind::Xenstore => Ok(Self::XenstoreStd(XenstoreStdPublisher)),
            PublisherKind::XenstoreRfc => Ok(Self::XenstoreRfc(XenstoreRfcPublisher)),
        }
    }

    pub async fn run(self, channel: flume::Receiver<GuestMetric>) {
        match self {
            AgentPublisher::Console(publisher) => publisher.run(channel).await,
            AgentPublisher::XenstoreRfc(publisher) => publisher.run(channel).await,
            AgentPublisher::XenstoreStd(publisher) => publisher.run(channel).await,
        }
    }
}
