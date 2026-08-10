//! Tokio broadcast binding for committed entity changes.

use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_entity::change_sink::EntityChangeSink;
use tokio::sync::broadcast;
use tracing::debug;

/// Publishes entity policy events through the broker runtime's broadcast channel.
pub struct BroadcastEntityChangeSink {
    sender: broadcast::Sender<ChangeEvent>,
}

impl BroadcastEntityChangeSink {
    /// Create a sink backed by the supplied broadcast sender.
    pub const fn new(sender: broadcast::Sender<ChangeEvent>) -> BroadcastEntityChangeSink {
        BroadcastEntityChangeSink { sender }
    }
}

impl EntityChangeSink for BroadcastEntityChangeSink {
    fn publish(&self, event: ChangeEvent) {
        if let Err(error) = self.sender.send(event) {
            debug!(%error, "entity change has no active receiver");
        }
    }
}
