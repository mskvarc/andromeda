//! Change-driven context source notifications (clauses 5.11.2.4 and 5.11.7).
//!
//! Keeps the compiled context source subscriptions of one tenant and, on
//! every subscription or registration change, decides which of them are
//! notified of which registrations from what each was already told, as the
//! tenant database records it ([`crate::csource_membership`]); the delivery
//! itself lives beside it.

use crate::{
    csource_delivery::{CsourceDeliveryOutcome, CsourceNotificationDelivery},
    csource_match_set::matching_registrations,
    csource_membership::{CsourceMembership, PlannedNotification},
    csource_transition::RegistrationChange,
    error::NotificationError,
    notified_registration_store::NotifiedRegistrationStore,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    notification::{CsrChangeEvent, CsrTriggerReason},
    registration::ContextSourceRegistration,
    subscription::SubscriptionStatus,
};
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{
    change::{CsourceSubscriptionChange, UpsertKind},
    compilation::compile_csource_subscription,
    compiled::CompiledCsrSubscription,
    csource_cache::CsourceSubscriptionCache,
    csource_matching::csource_subscription_is_live,
    csource_notification_cadence::CsourceNotificationCadence,
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use futures::{StreamExt, stream};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, error};

const CSR_NOTIFICATION_CONCURRENCY: usize = 20;

/// The registration a change event's snapshot carries, read back into its
/// typed form so it is matched exactly as a stored registration is.
fn snapshot_registration(snapshot: &Value, csr_id: &Identifier) -> Option<ContextSourceRegistration> {
    match serde_json::from_value(snapshot.clone()) {
        // clone: deserialisation consumes the document and the event still owns its snapshot
        Ok(registration) => Some(registration),
        Err(error) => {
            error!("CSR {} snapshot is not a registration: {}", csr_id, error);
            None
        }
    }
}

/// Applies context-source subscription and registration events to notification policy.
pub struct CsourceNotificationProcessor {
    target: ContextTarget,
    sub_repo: Arc<dyn SubscriptionStore>,
    csr_repo: Arc<dyn RegistrationStore>,
    cache: CsourceSubscriptionCache,
    membership: CsourceMembership,
    delivery: CsourceNotificationDelivery,
}

/// The stores and services one tenant's context source notification policy
/// reads, records and delivers through.
pub struct CsourceNotificationProcessorDeps {
    /// The live branch of the tenant.
    pub target: ContextTarget,
    /// The tenant's subscriptions.
    pub sub_repo: Arc<dyn SubscriptionStore>,
    /// The tenant's context source registrations.
    pub csr_repo: Arc<dyn RegistrationStore>,
    /// The registrations each subscription was notified of.
    pub notified: Arc<dyn NotifiedRegistrationStore>,
    /// The JSON-LD engine notifications are compacted with.
    pub engine: JsonLdEngine,
    /// The dispatcher every transport binding is reached through.
    pub transport: Arc<TransportDispatcher>,
    /// The tenant notifications are addressed under.
    pub tenant: TenantRef,
}

impl CsourceNotificationProcessor {
    /// Construct event evaluation over focused policy and delivery capabilities.
    #[must_use]
    pub fn new(deps: CsourceNotificationProcessorDeps) -> CsourceNotificationProcessor {
        let CsourceNotificationProcessorDeps {
            target,
            sub_repo,
            csr_repo,
            notified,
            engine,
            transport,
            tenant,
        } = deps;
        CsourceNotificationProcessor {
            target: target.clone(), // clone: the processor and its delivery half address the same branch.
            sub_repo: Arc::clone(&sub_repo),
            csr_repo,
            cache: CsourceSubscriptionCache::new(),
            membership: CsourceMembership::new(notified),
            delivery: CsourceNotificationDelivery::new(target, sub_repo, engine, transport, tenant),
        }
    }

    /// Reload compiled context-source subscriptions after startup or an event-stream gap.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Subscription`] when the subscription store cannot
    /// list the tenant's active subscriptions; the cache is then left as it was.
    pub async fn reload_cache(&self) -> Result<(), NotificationError> {
        let subscriptions = self.sub_repo.list_active(&self.target).await?;
        self.cache.clear();
        // A context-source notification is delivered as the change is applied
        // and never enqueues a delivery, so the surrogate key each stored
        // subscription carries stays unused on this path.
        for stored in subscriptions {
            let compiled = compile_csource_subscription(&stored.subscription);
            self.cache.insert(stored.subscription.id.clone(), Arc::new(compiled)); // clone: the map owns its key
        }
        Ok(())
    }

    /// Apply one committed context-source subscription lifecycle change.
    pub async fn apply_subscription_change(&self, change: CsourceSubscriptionChange) {
        match change {
            CsourceSubscriptionChange::Upserted { target, id, kind } => match self.sub_repo.get(&target, &id).await {
                Ok(Some(stored)) if stored.subscription.lifecycle.status == SubscriptionStatus::Active => {
                    let compiled = Arc::new(compile_csource_subscription(&stored.subscription));
                    self.cache.insert(id, Arc::clone(&compiled));
                    self.send_matching_csrs_for_subscription(&compiled, kind).await;
                }
                Ok(Some(_) | None) => self.cache.remove(&id),
                Err(e) => {
                    debug!("Failed to load CSR subscription {} for cache update: {}", id, e);
                }
            },
            CsourceSubscriptionChange::Deleted { id, .. } => self.cache.remove(&id),
        }
    }

    /// Notify `compiled_sub` of the registrations it currently selects that an
    /// upsert of `kind` owes it (clauses 5.11.2.4 and 5.11.3.4).
    ///
    /// Both bullets of clause 5.11.2.4 owe a notification "initially on
    /// subscription", and clause 5.11.3.4 owes one after every update, so this
    /// runs for a `timeInterval` subscription as well; what the cadence
    /// settles is only whether an empty match set is still worth sending.
    /// Clause 5.3.3 names the reason: `newlyMatching` "is used in the first
    /// notification".
    async fn send_matching_csrs_for_subscription(&self, compiled_sub: &CompiledCsrSubscription, kind: UpsertKind) {
        let sub_id = &compiled_sub.id;
        if !csource_subscription_is_live(compiled_sub) {
            return;
        }

        let matching = match matching_registrations(self.csr_repo.as_ref(), compiled_sub).await {
            Ok(matching) => matching,
            Err(error) => {
                error!("Failed to fetch CSRs for subscription {} notification: {}", sub_id, error);
                return;
            }
        };
        let plan = match self.membership.plan_upsert(compiled_sub, kind, &matching).await {
            Ok(plan) => plan,
            Err(error) => {
                error!("Failed to record the CSRs notified to subscription {}: {}", sub_id, error);
                return;
            }
        };
        let withdrawn = match kind {
            UpsertKind::Created => Vec::new(),
            UpsertKind::Updated => self.withdraw_unselected_records(compiled_sub, &matching).await,
        };
        let notified = self.membership.confirm(compiled_sub, self.current_states(&plan.carried).await).await;

        // Clause 5.11.2.4: a change-driven notification informs "about this
        // change" to a matching registration, so while nothing is to be
        // reported there is no change to report; a periodic one is due on its
        // schedule "independent of any changes to the set of Context Source
        // registrations" and goes out carrying whatever there is.
        if notified.is_empty() && CsourceNotificationCadence::of(compiled_sub).is_change_driven() {
            return;
        }

        let carried: Vec<&ContextSourceRegistration> = notified.iter().collect();
        let outcome = match self.delivery.notified_registrations(&carried, compiled_sub).await {
            Ok(data) => self.delivery.deliver(compiled_sub, data, CsrTriggerReason::NewlyMatching).await,
            Err(error) => {
                error!("Failed to serialize CSRs for subscription {}: {}", sub_id, error);
                CsourceDeliveryOutcome::NotSent
            }
        };
        // A notification that never reached the subscriber leaves the record as
        // the subscriber last saw it: what it newly recorded is withdrawn, so
        // those registrations are reported by their next change, and what an
        // update withdrew is recorded again.
        match outcome {
            CsourceDeliveryOutcome::NotSent => {
                self.membership.withdraw_unsent(sub_id, &plan.recorded).await;
                self.membership.restore(sub_id, &withdrawn).await;
            }
            CsourceDeliveryOutcome::Sent => {}
        }
    }

    /// Withdraw the records of an updated subscription that lie outside its
    /// current match set and that it no longer selects, returning those
    /// withdrawn.
    async fn withdraw_unselected_records(&self, compiled_sub: &CompiledCsrSubscription, matching: &[ContextSourceRegistration]) -> Vec<Identifier> {
        let beyond = match self.membership.recorded_beyond(&compiled_sub.id, matching).await {
            Ok(beyond) => beyond,
            Err(error) => {
                error!("Failed to read the CSRs notified to subscription {}: {}", compiled_sub.id, error);
                return Vec::new();
            }
        };
        let mut current = Vec::with_capacity(beyond.len());
        for identifier in beyond {
            match self.csr_repo.get(&identifier).await {
                Ok(state) => current.push((identifier, state)),
                // A record that cannot be re-read is kept rather than withdrawn
                // on a guess.
                Err(error) => error!("Failed to re-read CSR {} for subscription {}: {}", identifier, compiled_sub.id, error),
            }
        }
        self.membership.withdraw_unselected(compiled_sub, current).await
    }

    /// Each of `registrations` paired with its state as the store holds it
    /// now, or with its state as read when the store cannot be read, so a read
    /// failure never drops a registration the subscriber is owed.
    async fn current_states(&self, registrations: &[&ContextSourceRegistration]) -> Vec<(Identifier, Option<ContextSourceRegistration>)> {
        let mut current = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let state = match self.csr_repo.get(&registration.id).await {
                Ok(state) => state,
                Err(error) => {
                    error!("Failed to re-read CSR {} for its initial notification: {}", registration.id, error);
                    Some((*registration).clone()) // clone: the notification owns the state read with the match set
                }
            };
            current.push((registration.id.clone(), state)); // clone: the pair owns the identifier it withdraws by
        }
        current
    }

    /// Evaluate and deliver notifications for one committed registration change.
    pub async fn apply_registration_change(&self, event: CsrChangeEvent) {
        // A subscription and a subsequent CSR mutation may be committed by
        // different request tasks before the subscription-change broadcast is
        // observed here. Reconcile from the authoritative store before
        // matching the CSR so a late registration cannot outrun the compiled
        // companion subscription cache (clause 5.11.2.4).
        if let Err(error) = self.reload_cache().await {
            error!(
                "Failed to reload context-source subscriptions before CSR {} evaluation: {}",
                event.csr_id, error
            );
            return;
        }

        let change = RegistrationChange::from_event_reason(event.trigger_reason);
        let before = event.csr_snapshot.as_ref().and_then(|snapshot| snapshot_registration(snapshot, &event.csr_id));
        let after = match change {
            RegistrationChange::Created | RegistrationChange::Updated => match self.csr_repo.get(&event.csr_id).await {
                Ok(Some(csr)) => Some(csr),
                Ok(None) => {
                    // The deletion that removed it is reported by its own event.
                    debug!("CSR {} no longer exists, skipping notification", event.csr_id);
                    return;
                }
                Err(e) => {
                    error!("Failed to fetch CSR {} for notification: {}", event.csr_id, e);
                    return;
                }
            },
            RegistrationChange::Deleted => None,
        };

        let planned = self
            .membership
            .plan_registration_change(&self.cache.subscriptions(), change, &event.csr_id, before.as_ref(), after.as_ref())
            .await;
        stream::iter(planned)
            .for_each_concurrent(CSR_NOTIFICATION_CONCURRENCY, |notification| async move {
                self.send_planned(&notification).await;
            })
            .await;
    }

    /// Send one planned notification, trimmed to the information the
    /// subscription matches (clause 5.11.7), and restore the record when it
    /// was held back.
    async fn send_planned(&self, planned: &PlannedNotification<'_>) {
        let compiled_sub = planned.subscription.as_ref();
        let outcome = match self.delivery.notified_registrations(&[planned.registration], compiled_sub).await {
            Ok(data) => self.delivery.deliver(compiled_sub, data, planned.reason).await,
            Err(error) => {
                error!("Failed to serialize CSR for subscription {}: {}", compiled_sub.id, error);
                CsourceDeliveryOutcome::NotSent
            }
        };
        match outcome {
            CsourceDeliveryOutcome::Sent => {}
            CsourceDeliveryOutcome::NotSent => self.membership.undo(planned).await,
        }
    }
}
