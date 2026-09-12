//! Change-driven context source notifications (clauses 5.11.2.4 and 5.11.7).
//!
//! Keeps the compiled context source subscriptions of one tenant and, on
//! every subscription or registration change, decides which of them are
//! notified of which registrations; the delivery itself lives beside it.

use crate::{csource_delivery::CsourceNotificationDelivery, csource_match_set::matching_registrations, error::NotificationError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    notification::{CsrChangeEvent, CsrTriggerReason},
    registration::ContextSourceRegistration,
    subscription::SubscriptionStatus,
};
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{
    change::CsourceSubscriptionChange,
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
use std::{collections::HashSet, sync::Arc};
use tracing::{debug, error};

const CSR_NOTIFICATION_CONCURRENCY: usize = 20;

/// The registration a change event's snapshot carries, read back into its
/// typed form so it is matched exactly as a stored registration is.
fn snapshot_registration(snapshot: &Value, csr_id: &Uri) -> Option<ContextSourceRegistration> {
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
    delivery: CsourceNotificationDelivery,
}

impl CsourceNotificationProcessor {
    /// Construct event evaluation over focused policy and delivery capabilities.
    pub fn new(
        target: ContextTarget,
        sub_repo: Arc<dyn SubscriptionStore>,
        csr_repo: Arc<dyn RegistrationStore>,
        engine: JsonLdEngine,
        transport: Arc<TransportDispatcher>,
        tenant: TenantRef,
    ) -> CsourceNotificationProcessor {
        CsourceNotificationProcessor {
            target: target.clone(), // clone: the processor and its delivery half address the same branch.
            sub_repo: Arc::clone(&sub_repo),
            csr_repo,
            cache: CsourceSubscriptionCache::new(),
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
            CsourceSubscriptionChange::Upserted { target, id } => {
                match self.sub_repo.get(&target, &id).await {
                    Ok(Some(stored)) => {
                        let compiled = if stored.subscription.lifecycle.status == SubscriptionStatus::Active {
                            Some(Arc::new(compile_csource_subscription(&stored.subscription)))
                        } else {
                            None
                        };

                        if let Some(ref c) = compiled {
                            self.cache.insert(id.clone(), Arc::clone(c)); // clone: the map owns its key
                        } else {
                            self.cache.remove(&id);
                        }

                        // Per spec 5.11.3: after updating a subscription, send a notification
                        // with all currently matching Context Source Registrations.
                        if let Some(compiled_sub) = compiled {
                            self.send_matching_csrs_for_subscription(&compiled_sub).await;
                        }
                    }
                    Ok(None) => {
                        self.cache.remove(&id);
                    }
                    Err(e) => {
                        debug!("Failed to load CSR subscription {} for cache update: {}", id, e);
                    }
                }
            }
            CsourceSubscriptionChange::Deleted { id, .. } => {
                self.cache.remove(&id);
            }
        }
    }

    /// Notify `compiled_sub` of every registration it currently selects
    /// (clauses 5.11.2.4 and 5.11.3.4).
    ///
    /// Both bullets of clause 5.11.2.4 owe a notification "initially on
    /// subscription", and clause 5.11.3.4 owes one after every update, so this
    /// runs for a `timeInterval` subscription as well; what the cadence
    /// settles is only whether an empty match set is still worth sending.
    /// Clause 5.3.3 names the reason: `newlyMatching` "is used in the first
    /// notification".
    async fn send_matching_csrs_for_subscription(&self, compiled_sub: &CompiledCsrSubscription) {
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

        // Clause 5.11.2.4: a change-driven notification informs "about this
        // change" to a matching registration, so while nothing matches there
        // is no change to report; a periodic one is due on its schedule
        // "independent of any changes to the set of Context Source
        // registrations" and goes out carrying whatever there is.
        if matching.is_empty() && CsourceNotificationCadence::of(compiled_sub).is_change_driven() {
            return;
        }

        let notified: Vec<&ContextSourceRegistration> = matching.iter().collect();
        let data = match self.delivery.notified_registrations(&notified, compiled_sub).await {
            Ok(data) => data,
            Err(error) => {
                error!("Failed to serialize CSRs for subscription {}: {}", sub_id, error);
                return;
            }
        };
        self.delivery.deliver(compiled_sub, data, CsrTriggerReason::NewlyMatching).await;
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
        // For Updated events with a pre-update snapshot, detect subscriptions that
        // no longer match and send noLongerMatching notifications.
        if event.trigger_reason == CsrTriggerReason::Updated
            && let Some(ref snapshot) = event.csr_snapshot
        {
            self.handle_csr_update_with_snapshot(&event, snapshot).await;
            return;
        }

        // Fetch the CSR data (use snapshot for deletes)
        let csr_data = match event.trigger_reason {
            CsrTriggerReason::NoLongerMatching => event.csr_snapshot.as_ref().and_then(|snapshot| snapshot_registration(snapshot, &event.csr_id)),
            CsrTriggerReason::NewlyMatching | CsrTriggerReason::Updated => match self.csr_repo.get(&event.csr_id).await {
                Ok(Some(csr)) => Some(csr),
                Ok(None) => {
                    debug!("CSR {} no longer exists, skipping notification", event.csr_id);
                    return;
                }
                Err(e) => {
                    error!("Failed to fetch CSR {} for notification: {}", event.csr_id, e);
                    return;
                }
            },
        };

        let Some(csr_data) = csr_data else {
            return;
        };

        let matching = self.cache.find_matching(&csr_data);

        if matching.is_empty() {
            return;
        }

        let trigger_reason = event.trigger_reason;
        let csr_data = &csr_data;
        let _results: Vec<()> = stream::iter(matching)
            .map(|compiled_sub| async move {
                self.send_csr_notification(&compiled_sub, csr_data, trigger_reason).await;
            })
            .buffer_unordered(CSR_NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
    }

    /// Handle a CSR update where we have a pre-update snapshot.
    /// Sends "updated" to subscriptions that still match, and "noLongerMatching"
    /// to subscriptions that matched the old CSR but not the new one.
    async fn handle_csr_update_with_snapshot(&self, event: &CsrChangeEvent, pre_update: &Value) {
        let Some(pre_update) = snapshot_registration(pre_update, &event.csr_id) else {
            return;
        };
        let pre_update = &pre_update;
        // Fetch post-update CSR from DB
        let post_update = match self.csr_repo.get(&event.csr_id).await {
            Ok(Some(csr)) => csr,
            Ok(None) => return,
            Err(e) => {
                error!("Failed to fetch CSR {} for update notification: {}", event.csr_id, e);
                return;
            }
        };
        let post_update = &post_update;

        let matched_before = self.cache.find_matching(pre_update);
        let matched_after = self.cache.find_matching(post_update);

        let after_ids: HashSet<&Uri> = matched_after.iter().map(|subscription| &subscription.id).collect();
        let before_ids: HashSet<&Uri> = matched_before.iter().map(|subscription| &subscription.id).collect();

        // Subscriptions that still match → "updated"
        let updated_subs: Vec<Arc<CompiledCsrSubscription>> = matched_after.iter().filter(|sub| before_ids.contains(&sub.id)).cloned().collect();
        let _updated: Vec<()> = stream::iter(updated_subs)
            .map(|sub| async move {
                self.send_csr_notification(&sub, post_update, CsrTriggerReason::Updated).await;
            })
            .buffer_unordered(CSR_NOTIFICATION_CONCURRENCY)
            .collect()
            .await;

        // Subscriptions that matched before but not after → "noLongerMatching"
        let no_longer_matching_subs: Vec<Arc<CompiledCsrSubscription>> = matched_before.iter().filter(|sub| !after_ids.contains(&sub.id)).cloned().collect();
        let _no_longer_matching: Vec<()> = stream::iter(no_longer_matching_subs)
            .map(|sub| async move {
                self.send_csr_notification(&sub, pre_update, CsrTriggerReason::NoLongerMatching).await;
            })
            .buffer_unordered(CSR_NOTIFICATION_CONCURRENCY)
            .collect()
            .await;

        // Subscriptions that match after but not before → "newlyMatching" (from update)
        let newly_matching_subs: Vec<Arc<CompiledCsrSubscription>> = matched_after.iter().filter(|sub| !before_ids.contains(&sub.id)).cloned().collect();
        let _newly_matching: Vec<()> = stream::iter(newly_matching_subs)
            .map(|sub| async move {
                self.send_csr_notification(&sub, post_update, CsrTriggerReason::NewlyMatching).await;
            })
            .buffer_unordered(CSR_NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
    }

    /// Notify `compiled_sub` of one registration, trimmed to the information
    /// it matches (clause 5.11.7).
    async fn send_csr_notification(&self, compiled_sub: &CompiledCsrSubscription, csr_data: &ContextSourceRegistration, trigger_reason: CsrTriggerReason) {
        let data = match self.delivery.notified_registrations(&[csr_data], compiled_sub).await {
            Ok(data) => data,
            Err(error) => {
                error!("Failed to serialize CSR for subscription {}: {}", compiled_sub.id, error);
                return;
            }
        };
        self.delivery.deliver(compiled_sub, data, trigger_reason).await;
    }
}
