//! What a scene does with the companion and the copies owed the receiver the
//! service serves.

use crate::{error::SubscriptionForwardingError, reassembly_harness::ReassemblyScene};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

impl ReassemblyScene {
    /// Adopt the receiver the service serves, as the tenant does before it
    /// serves a request.
    pub(crate) async fn adopt_receiver(&self) -> Result<(), SubscriptionForwardingError> {
        self.service.adopt_receiver().await
    }

    /// Run one pass of the background update of the companions and copies
    /// owed the current receiver.
    pub(crate) async fn redirect_overdue(&self) -> Result<(), SubscriptionForwardingError> {
        self.service.redirect_overdue().await
    }

    /// The notification endpoint URIs the updates of the subscription `id`
    /// wrote to the store, in the order they came.
    pub(crate) fn endpoints_written(&self, id: &Identifier) -> Vec<String> {
        self.subscriptions.endpoints_written(id)
    }
}
