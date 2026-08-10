//! Bounded context-source registration expiry policy.

use crate::{RegistrationChangeSink, error::RegistrationError, store::RegistrationStore};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::{CsrChangeEvent, CsrTriggerReason};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Result of one bounded registration expiry execution.
#[derive(Debug, Eq, PartialEq)]
pub struct RegistrationExpiryOutcome {
    /// Identifiers deleted by clause 5.9.2.4 expiry semantics.
    pub expired: Vec<Uri>,
}

/// Independently scheduled semantic deletion of expired registrations.
pub struct RegistrationExpiry {
    store: Arc<dyn RegistrationStore>,
    change_sink: Option<Arc<dyn RegistrationChangeSink>>,
}

impl RegistrationExpiry {
    /// Construct expiry over registration-owned persistence and change ports.
    pub const fn new(store: Arc<dyn RegistrationStore>, change_sink: Option<Arc<dyn RegistrationChangeSink>>) -> RegistrationExpiry {
        RegistrationExpiry { store, change_sink }
    }

    /// Delete every registration due at `cutoff` and publish its pre-deletion snapshot.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<RegistrationExpiryOutcome, RegistrationError> {
        let registrations = self.store.delete_expired(cutoff).await?;
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
