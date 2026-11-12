//! Context-source registration deletion policy.

use crate::{RegistrationService, error::RegistrationError};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::{CsrChange, CsrChangeEvent};

impl RegistrationService {
    /// Delete a registration by URI.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::NotFound`] when no registration carries
    /// `id`, and [`RegistrationError::Persistence`] when the store rejects the
    /// read or the deletion. A removal hook that cannot
    /// capture or finish its teardown is logged rather than returned: the
    /// registration is deleted either way.
    pub async fn delete(&self, id: &Identifier) -> Result<(), RegistrationError> {
        let existing = self.repo.get(id).await?;
        // Captured before the row goes: the store deletes the forwarding
        // records together with the registration.
        let teardown = match (self.removal_hook.as_deref(), existing.as_ref()) {
            (Some(hook), Some(registration)) => hook
                .teardown_of(registration)
                .await
                .inspect_err(|error| tracing::warn!(registration = %id, %error, "registration removal teardown could not be captured"))
                .ok(),
            (Some(_) | None, None) | (None, Some(_)) => None,
        };
        let revision = self.repo.delete(id).await?.require_applied(id)?;
        self.census.release(1);
        if let (Some(hook), Some(teardown)) = (self.removal_hook.as_deref(), teardown.as_ref()) {
            hook.removed(teardown).await;
        }
        self.emit_csr_change(CsrChangeEvent {
            csr_id: id.clone(),
            change: CsrChange::Deleted {
                before: existing.map(Box::new),
            },
            revision,
        })
        .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::RegistrationError,
        fixed_store::{FixedStore, registration},
        registration_census::RegistrationCensus,
        removal_hook::{ForwardedSubscriptionCopy, RegistrationRemovalHook, RegistrationTeardown},
        service_fixture::{offline_engine, service_counting, service_over},
        store::RegistrationStore,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use futures::future::BoxFuture;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    const REGISTRATION_ID: &str = "urn:ngsi-ld:ContextSourceRegistration:removed";

    /// One observable step of a registration removal, with how many rows the
    /// store had deleted when it happened.
    #[derive(Debug, PartialEq, Eq)]
    enum RemovalStep {
        /// The hook read the teardown.
        Captured { rows_deleted: usize },
        /// The hook was handed the teardown back.
        Removed {
            rows_deleted: usize,
            forwarded: Vec<ForwardedSubscriptionCopy>,
        },
    }

    /// A hook that captures one fixed forwarded copy and logs every step
    /// against the store's deletion log.
    struct RecordingHook {
        store: Arc<FixedStore>,
        steps: Mutex<Vec<RemovalStep>>,
    }

    fn forwarded_copy() -> ForwardedSubscriptionCopy {
        ForwardedSubscriptionCopy {
            local_subscription: "urn:ngsi-ld:Subscription:local".parse().expect("valid URI"),
            remote_subscription: "urn:ngsi-ld:Subscription:remote".parse().expect("valid URI"),
        }
    }

    impl RegistrationRemovalHook for RecordingHook {
        fn teardown_of<'life0, 'life1, 'async_trait>(
            &'life0 self,
            registration: &'life1 ContextSourceRegistration,
        ) -> BoxFuture<'async_trait, Result<RegistrationTeardown, RegistrationError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
        {
            self.steps.lock().expect("step log").push(RemovalStep::Captured {
                rows_deleted: self.store.deleted().len(),
            });
            let teardown = RegistrationTeardown {
                registration: registration.clone(),
                forwarded: vec![forwarded_copy()],
            };
            Box::pin(async { Ok(teardown) })
        }

        fn removed<'life0, 'life1, 'async_trait>(&'life0 self, teardown: &'life1 RegistrationTeardown) -> BoxFuture<'async_trait, ()>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
        {
            self.steps.lock().expect("step log").push(RemovalStep::Removed {
                rows_deleted: self.store.deleted().len(),
                forwarded: teardown.forwarded.clone(),
            });
            Box::pin(async {})
        }
    }

    /// Clause 5.9.5 with clause 5.8.5.4: the forwarded copies cascade with the
    /// registration row, so they are captured before the row goes and handed
    /// to the hook, unchanged, only after it has.
    #[tokio::test]
    async fn a_deletion_captures_the_forwarded_copies_before_the_row_and_tears_them_down_after() {
        let store = Arc::new(FixedStore::new(vec![registration(
            REGISTRATION_ID,
            "inclusive",
            &json!([{"entities": [{"type": "https://example.org/Vehicle"}]}]),
        )]));
        let hook = Arc::new(RecordingHook {
            store: Arc::clone(&store),
            steps: Mutex::new(Vec::new()),
        });
        let service = service_over(Arc::clone(&store) as Arc<dyn RegistrationStore>, offline_engine(), None)
            .with_removal_hook(Arc::clone(&hook) as Arc<dyn RegistrationRemovalHook>);

        let id: Identifier = REGISTRATION_ID.parse().expect("valid URI");
        service.delete(&id).await.expect("the registration is deleted");

        assert_eq!(store.deleted(), vec![id]);
        let steps = hook.steps.lock().expect("step log");
        assert_eq!(
            *steps,
            vec![
                RemovalStep::Captured { rows_deleted: 0 },
                RemovalStep::Removed {
                    rows_deleted: 1,
                    forwarded: vec![forwarded_copy()],
                },
            ]
        );
    }

    /// Deleting an unknown registration is `ResourceNotFound` (clause 5.9.5),
    /// and the hook is never asked for a teardown of a registration that did
    /// not exist.
    #[tokio::test]
    async fn deleting_an_unknown_registration_asks_the_hook_for_nothing() {
        let store = Arc::new(FixedStore::new(Vec::new()));
        let hook = Arc::new(RecordingHook {
            store: Arc::clone(&store),
            steps: Mutex::new(Vec::new()),
        });
        let service = service_over(Arc::clone(&store) as Arc<dyn RegistrationStore>, offline_engine(), None)
            .with_removal_hook(Arc::clone(&hook) as Arc<dyn RegistrationRemovalHook>);

        let id: Identifier = "urn:ngsi-ld:ContextSourceRegistration:unknown".parse().expect("valid URI");
        let outcome = service.delete(&id).await;

        assert!(matches!(outcome, Err(RegistrationError::NotFound { .. })), "{outcome:?}");
        assert!(hook.steps.lock().expect("step log").is_empty());
    }

    #[tokio::test]
    async fn a_deleted_registration_is_no_longer_counted() {
        let store = Arc::new(FixedStore::new(vec![registration(
            REGISTRATION_ID,
            "inclusive",
            &json!([{"entities": [{"type": "https://example.org/Vehicle"}]}]),
        )]));
        let census = RegistrationCensus::new(1);
        let service = service_counting(store as Arc<dyn RegistrationStore>, census.clone(), offline_engine(), None);

        let id: Identifier = REGISTRATION_ID.parse().expect("valid URI");
        service.delete(&id).await.expect("the registration is deleted");

        assert!(census.holds_none());
    }

    /// A deletion that finds nothing removes nothing, so every registration
    /// the store holds stays counted.
    #[tokio::test]
    async fn deleting_an_unknown_registration_keeps_the_count() {
        let census = RegistrationCensus::new(1);
        let service = service_counting(
            Arc::new(FixedStore::new(Vec::new())) as Arc<dyn RegistrationStore>,
            census.clone(),
            offline_engine(),
            None,
        );

        let id: Identifier = "urn:ngsi-ld:ContextSourceRegistration:unknown".parse().expect("valid URI");
        let outcome = service.delete(&id).await;

        assert!(matches!(outcome, Err(RegistrationError::NotFound { .. })), "{outcome:?}");
        assert_eq!(census.release(0), 1);
    }
}
