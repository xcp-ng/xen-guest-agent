use futures::{channel::mpsc, SinkExt};
use tokio::task::JoinSet;

use guest_metrics::{plugin::GuestAgentPlugin, GuestMetric};

use crate::{plugins::NetworkPlugin, publisher::AgentPublisher, GuestAgentConfig};

pub async fn run(config: GuestAgentConfig) -> anyhow::Result<()> {
    let mut set: JoinSet<()> = JoinSet::new();

    let (mut tx, rx) = mpsc::channel(4);
    let publisher = AgentPublisher::new(config.publisher)?;

    set.spawn(publisher.run(rx));

    // Remove old entries from previous agent to avoid having unknown
    // interfaces. We will repopulate existing ones immediatly.
    tx.send(GuestMetric::CleanupIfaces).await?;

    set.spawn(provider_os::OsInfoPlugin.run(tx.clone()));
    set.spawn(provider_memory::MemoryPlugin.run(tx.clone()));
    set.spawn(NetworkPlugin::new(config.network)?.run(tx.clone()));

    println!("{:?}", set.join_all().await);
    Ok(())
}
