use std::future::Future;

use crate::GuestMetric;

pub trait GuestAgentPlugin {
    fn run(self, channel: flume::Sender<GuestMetric>) -> impl Future<Output = ()> + Send;
}

pub trait GuestAgentPublisher {
    fn run(self, channel: flume::Receiver<GuestMetric>) -> impl Future<Output = ()> + Send;
}
