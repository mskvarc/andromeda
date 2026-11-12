//! Bounded context-source registration expiry policy.

use crate::{
    RegistrationChangeSink,
    error::RegistrationError,
    registration_census::RegistrationCensus,
    removal_hook::{RegistrationRemovalHook, RegistrationTeardown},
    store::{RegistrationStore, expired_deletion::ExpiredDeletion},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::{CsrChange, CsrChangeEvent};
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
    /// How many registrations the tenant holds, which the sweep's deletions
    /// lower.
    census: RegistrationCensus,
    /// Where each deletion is announced.
    change_sink: Option<Arc<dyn RegistrationChangeSink>>,
    /// The distributed side of each removal, absent when forwarding is off.
    removal_hook: Option<Arc<dyn RegistrationRemovalHook>>,
}

impl RegistrationExpiry {
    /// Construct expiry over registration-owned persistence, the census of
    /// what it holds, and the change and removal ports.
    pub const fn new(
        store: Arc<dyn RegistrationStore>,
        census: RegistrationCensus,
        change_sink: Option<Arc<dyn RegistrationChangeSink>>,
        removal_hook: Option<Arc<dyn RegistrationRemovalHook>>,
    ) -> RegistrationExpiry {
        RegistrationExpiry {
            store,
            census,
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
    /// delete the due registrations.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<RegistrationExpiryOutcome, RegistrationError> {
        let teardowns = self.capture_teardowns(cutoff).await?;
        let ExpiredDeletion { revision, registrations } = self.store.delete_expired(cutoff).await?;
        self.census.release(registrations.len());
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
            let identifier = registration.id.clone();
            if let Some(ref sink) = self.change_sink {
                sink.publish(CsrChangeEvent {
                    csr_id: identifier.clone(),
                    change: CsrChange::Deleted {
                        before: Some(Box::new(registration)),
                    },
                    revision,
                })
                .await;
            }
            expired.push(identifier);
        }
        Ok(RegistrationExpiryOutcome { expired })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        expiry::RegistrationExpiry,
        fixed_store::{FixedStore, registration},
        registration_census::RegistrationCensus,
        store::RegistrationStore,
    };
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::Arc;

    /// A sweep over a store whose expiry deletes `count` registrations.
    fn sweep_deleting(count: usize, census: &RegistrationCensus) -> RegistrationExpiry {
        let information = json!([{"entities": [{"type": "https://example.org/Vehicle"}]}]);
        let expired = (0..count)
            .map(|index| registration(&format!("urn:ngsi-ld:ContextSourceRegistration:expired-{index}"), "inclusive", &information))
            .collect();
        let store = FixedStore::new(Vec::new()).expiring(expired);
        RegistrationExpiry::new(Arc::new(store) as Arc<dyn RegistrationStore>, census.clone(), None, None)
    }

    fn cutoff() -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse().expect("a valid instant")
    }

    #[tokio::test]
    async fn every_registration_the_sweep_deletes_is_no_longer_counted() {
        let census = RegistrationCensus::new(3);
        let outcome = sweep_deleting(2, &census).execute(cutoff()).await.expect("the sweep runs");
        assert_eq!(outcome.expired.len(), 2);
        assert_eq!(census.release(0), 1);
    }

    #[tokio::test]
    async fn a_sweep_deleting_nothing_keeps_the_count() {
        let census = RegistrationCensus::new(1);
        sweep_deleting(0, &census).execute(cutoff()).await.expect("the sweep runs");
        assert_eq!(census.release(0), 1);
    }
}
