use std::future::Future;

use futures::channel::mpsc;

use crate::GuestMetric;

pub trait GuestAgentPlugin {
    fn run(self, channel: mpsc::Sender<GuestMetric>) -> impl Future<Output = ()> + Send;
}

pub trait GuestAgentPublisher {
    fn run(self, channel: mpsc::Receiver<GuestMetric>) -> impl Future<Output = ()> + Send;
}
