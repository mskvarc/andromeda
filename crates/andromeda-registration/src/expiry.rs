//! Bounded context-source registration expiry policy.

use crate::{
    RegistrationChangeSink,
    error::RegistrationError,
    removal_hook::{RegistrationRemovalHook, RegistrationTeardown},
    store::RegistrationStore,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::{CsrChangeEvent, CsrTriggerReason};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Result of one bounded registration expiry execution.
#[derive(Debug, Eq, PartialEq)]
pub struct RegistrationExpiryOutcome {
    /// Identifiers deleted by clause 5.9.2.4 expiry semantics.
    pub expired: Vec<Identifier>,
}

/// Independently scheduled semantic deletion of expired registrations
/// (clause 5.9.2.4).
///
/// The scheduler that owns the cadence lives in broker composition; this type
/// is one sweep. Between sweeps the store's reads already hide a registration
/// past its `expiresAt`, so the sweep is what frees the row and announces the
/// deletion, not what makes it invisible.
pub struct RegistrationExpiry {
    /// The registration store the sweep deletes from.
    store: Arc<dyn RegistrationStore>,
    /// Where each deletion is announced.
    change_sink: Option<Arc<dyn RegistrationChangeSink>>,
    /// The distributed side of each removal, absent when forwarding is off.
    removal_hook: Option<Arc<dyn RegistrationRemovalHook>>,
}

impl RegistrationExpiry {
    /// Construct expiry over registration-owned persistence, change and
    /// removal ports.
    pub const fn new(
        store: Arc<dyn RegistrationStore>,
        change_sink: Option<Arc<dyn RegistrationChangeSink>>,
        removal_hook: Option<Arc<dyn RegistrationRemovalHook>>,
    ) -> RegistrationExpiry {
        RegistrationExpiry {
            store,
            change_sink,
            removal_hook,
        }
    }

    /// Capture the teardown of every registration due at `cutoff`, before the
    /// deletion removes the forwarding records it reads. A registration whose
    /// teardown cannot be captured is logged and expired all the same.
    async fn capture_teardowns(&self, cutoff: DateTime<Utc>) -> Result<Vec<RegistrationTeardown>, RegistrationError> {
        let Some(hook) = self.removal_hook.as_deref() else {
            return Ok(Vec::new());
        };
        let mut teardowns = Vec::new();
        for registration in self.store.find_expired(cutoff).await? {
            match hook.teardown_of(&registration).await {
                Ok(teardown) => teardowns.push(teardown),
                Err(error) => tracing::warn!(registration = %registration.id, %error, "registration removal teardown could not be captured"),
            }
        }
        Ok(teardowns)
    }

    /// Delete every registration due at `cutoff` and publish its pre-deletion snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::Persistence`] when the store cannot read or
    /// delete the due registrations, and [`RegistrationError::Representation`] when a
    /// deleted registration cannot be serialised into the snapshot the change
    /// event carries.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<RegistrationExpiryOutcome, RegistrationError> {
        let teardowns = self.capture_teardowns(cutoff).await?;
        let registrations = self.store.delete_expired(cutoff).await?;
        if let Some(hook) = self.removal_hook.as_deref() {
            for teardown in teardowns
                .iter()
                .filter(|teardown| registrations.iter().any(|deleted| deleted.id == teardown.registration.id))
            {
                hook.removed(teardown).await;
            }
        }
        let mut expired = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let snapshot = serde_json::to_value(&registration)?;
            let identifier = registration.id;
            if let Some(ref sink) = self.change_sink {
                sink.publish(CsrChangeEvent {
                    csr_id: identifier.clone(), // clone: outcome and event independently own the identifier.
                    trigger_reason: CsrTriggerReason::NoLongerMatching,
                    csr_snapshot: Some(snapshot),
                });
            }
            expired.push(identifier);
        }
        Ok(RegistrationExpiryOutcome { expired })
    }
}
