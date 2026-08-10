//! Tokio broadcast binding for registration change events.

use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_registration::RegistrationChangeSink;
use tokio::sync::broadcast;
use tracing::debug;

/// Publishes registration policy events through the broker runtime's broadcast channel.
pub struct BroadcastRegistrationChangeSink {
    sender: broadcast::Sender<CsrChangeEvent>,
}

impl BroadcastRegistrationChangeSink {
    /// Create a sink backed by the supplied broadcast sender.
    pub const fn new(sender: broadcast::Sender<CsrChangeEvent>) -> BroadcastRegistrationChangeSink {
        BroadcastRegistrationChangeSink { sender }
    }
}

impl RegistrationChangeSink for BroadcastRegistrationChangeSink {
    fn publish(&self, event: CsrChangeEvent) {
        if let Err(error) = self.sender.send(event) {
            debug!(%error, "registration change has no active receiver");
        }
    }
}
