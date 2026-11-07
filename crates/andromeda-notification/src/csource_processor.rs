//! Change-driven context source notifications (clauses 5.11.2.4 and 5.11.7).
//!
//! Keeps the compiled context source subscriptions of one tenant and, on
//! every subscription or registration change, decides which of them are
//! notified of which registrations from what each was already told, as the
//! tenant database records it ([`crate::csource_membership`]); the delivery
//! itself lives beside it.
//!
//! A change is reached after later writes may have committed, so each one is
//! evaluated against the state it found, as the change revisions order them
//! ([`crate::csource_change_order`]): a registration change skips every
//! subscription written after it, and a subscription write is notified once,
//! by whichever of its changes claims the latest write first.

use crate::{
    csource_change_order::{CsourceChangeOrder, LatestWriteClaim},
    csource_delivery::{CsourceDeliveryOutcome, CsourceNotificationDelivery},
    csource_match_set::matching_registrations,
    csource_membership::{CsourceMembership, PlannedNotification},
    csource_transition::ChangeStates,
    error::NotificationError,
    notified_registration_store::NotifiedRegistrationStore,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    csource_change_revision::CsourceChangeRevision,
    notification::{CsrChangeEvent, CsrTriggerReason},
    registration::ContextSourceRegistration,
};
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{
    change::{CsourceSubscriptionChange, UpsertKind},
    compilation::compile_csource_subscription,
    compiled::CompiledCsrSubscription,
    csource_cache::CsourceSubscriptionCache,
    csource_notification_cadence::CsourceNotificationCadence,
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use futures::{StreamExt, stream};
use std::sync::Arc;
use tracing::{debug, error};

/// Planned notifications one registration change sends at once.
const CSR_NOTIFICATION_CONCURRENCY: usize = 20;

/// Applies context-source subscription and registration events to notification policy.
pub struct CsourceNotificationProcessor {
    /// The live branch of the tenant the subscriptions are listed on.
    target: ContextTarget,
    /// The tenant's subscriptions, read to refresh the cache.
    sub_repo: Arc<dyn SubscriptionStore>,
    /// The tenant's registrations, matched against each subscription.
    csr_repo: Arc<dyn RegistrationStore>,
    /// The write order that decides which change notifies a subscription.
    order: Arc<dyn CsourceChangeOrder>,
    /// The compiled active context source subscriptions, keyed by id.
    cache: CsourceSubscriptionCache,
    /// The recorded registrations each subscription was already notified of.
    membership: CsourceMembership,
    /// Renders and sends the planned notifications.
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
    /// The order the tenant's registrations and subscriptions were written in.
    pub order: Arc<dyn CsourceChangeOrder>,
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
            order,
            engine,
            transport,
            tenant,
        } = deps;
        CsourceNotificationProcessor {
            target: target.clone(), // clone: the processor and its delivery half address the same branch.
            sub_repo: Arc::clone(&sub_repo),
            csr_repo,
            order,
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
    ///
    /// The latest write is claimed before the subscription is read, and
    /// confirmed still latest after: the state read is then exactly the one
    /// the claimed write left. A write that commits in between is notified by
    /// its own change.
    pub async fn apply_subscription_change(&self, change: CsourceSubscriptionChange) {
        match change {
            CsourceSubscriptionChange::Upserted { target, id, kind } => {
                let owed = self.claim_owed_write(&id).await;
                match self.sub_repo.get(&target, &id).await {
                    Ok(Some(stored)) if stored.subscription.lifecycle.standing.notifies_at(Utc::now()) => {
                        let compiled = Arc::new(compile_csource_subscription(&stored.subscription));
                        self.cache.insert(id, Arc::clone(&compiled));
                        if let Some(revision) = owed
                            && self.is_latest_write(&compiled.id, revision).await
                        {
                            self.send_matching_csrs_for_subscription(&compiled, kind).await;
                        }
                    }
                    Ok(Some(_) | None) => self.cache.remove(&id),
                    Err(e) => {
                        debug!("Failed to load CSR subscription {} for cache update: {}", id, e);
                    }
                }
            }
            CsourceSubscriptionChange::Deleted { id, .. } => self.cache.remove(&id),
        }
    }

    /// The revision of `id`'s latest write when this change is the one that
    /// notifies it, or `None` when another change already did.
    async fn claim_owed_write(&self, id: &Identifier) -> Option<CsourceChangeRevision> {
        match self.order.claim_latest_write(id).await {
            Ok(LatestWriteClaim::Claimed(revision)) => Some(revision),
            Ok(LatestWriteClaim::Settled) => None,
            Err(error) => {
                error!("Failed to claim the latest write of CSR subscription {}: {}", id, error);
                None
            }
        }
    }

    /// Whether the write that drew `revision` is still `id`'s latest.
    async fn is_latest_write(&self, id: &Identifier, revision: CsourceChangeRevision) -> bool {
        match self.order.latest_write(id).await {
            Ok(latest) => latest == Some(revision),
            Err(error) => {
                error!("Failed to read the latest write of CSR subscription {}: {}", id, error);
                false
            }
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
        if !compiled_sub.standing.notifies_at(Utc::now()) {
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
        let current = self.recorded_beyond_states(compiled_sub, matching).await;
        self.membership.withdraw_unselected(compiled_sub, &current).await
    }

    /// Each registration recorded for `compiled_sub` outside `matching`, paired
    /// with its state as the store holds it now.
    async fn recorded_beyond_states(
        &self,
        compiled_sub: &CompiledCsrSubscription,
        matching: &[ContextSourceRegistration],
    ) -> Vec<(Identifier, Option<ContextSourceRegistration>)> {
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
        current
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
        // A subscription may have been written by another request task, or by
        // another broker, whose change this worker has not seen yet. The cache
        // is reconciled from the store, and every subscription written after
        // the registration change is then left out: clause 5.11.2.4 notifies a
        // change of a registration that matched the subscription when it was
        // made, and a later write's own notification reports the change.
        if let Err(error) = self.reload_cache().await {
            error!(
                "Failed to reload context-source subscriptions before CSR {} evaluation: {}",
                event.csr_id, error
            );
            return;
        }
        let Some(found) = self.subscriptions_found_by(event.revision).await else {
            return;
        };

        let states = ChangeStates::of(&event.change);
        let planned = self
            .membership
            .plan_registration_change(&found, states.change, event.revision, &event.csr_id, states.before, states.after)
            .await;
        stream::iter(planned)
            .for_each_concurrent(CSR_NOTIFICATION_CONCURRENCY, |notification| async move {
                self.send_planned(&notification).await;
            })
            .await;
    }

    /// The cached subscriptions as a registration change that drew `revision`
    /// found them: those written after it are left out, or `None` when the
    /// order cannot be read.
    async fn subscriptions_found_by(&self, revision: CsourceChangeRevision) -> Option<Vec<Arc<CompiledCsrSubscription>>> {
        match self.order.written_after(revision).await {
            Ok(rewritten) => Some(
                self.cache
                    .subscriptions()
                    .into_iter()
                    .filter(|subscription| !rewritten.contains(&subscription.id))
                    .collect(),
            ),
            Err(error) => {
                error!("Failed to read the CSR subscriptions written after revision {}: {}", revision, error);
                None
            }
        }
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
