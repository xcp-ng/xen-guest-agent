use event_listener::{Event, EventListener};
use futures::{FutureExt, Stream};
use smol::Executor;
use std::{
    cell::RefCell,
    future::Future,
    sync::Arc,
    task::{ready, Poll},
};
use xenstore_rs::smol::XsSmol;

use crate::{vif::PlatformVifDetector, GuestMetric};

pub struct Shared {
    /// Triggered when a live migration occurs
    pub live_migration_event: Event<()>,
    pub executor: Executor<'static>,
    pub xs: Option<XsSmol<'static>>,
    pub vif_detector: PlatformVifDetector,
}

pub struct LiveMigrationStream<'a> {
    shared: &'a Shared,
    listener: RefCell<EventListener<()>>,
}

impl Stream for LiveMigrationStream<'_> {
    type Item = ();

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        ready!(self.listener.borrow_mut().poll_unpin(cx));
        self.listener
            .replace(self.shared.live_migration_event.listen());
        Poll::Ready(Some(()))
    }
}

impl Shared {
    pub fn live_migration_stream(&self) -> LiveMigrationStream<'_> {
        LiveMigrationStream {
            shared: &self,
            listener: RefCell::new(self.live_migration_event.listen()),
        }
    }
}

pub trait GuestAgentPlugin {
    fn run(
        self,
        shared: Arc<Shared>,
        channel: flume::Sender<GuestMetric>,
    ) -> impl Future<Output = ()> + Send;
}

pub trait GuestAgentPublisher {
    fn run(
        self,
        shared: Arc<Shared>,
        channel: flume::Receiver<GuestMetric>,
    ) -> impl Future<Output = ()> + Send;
}
