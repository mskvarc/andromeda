//! Change-driven context source notifications (clauses 5.11.2.4 and 5.11.7).
//!
//! Keeps the compiled context source subscriptions of one tenant and, on
//! every subscription or registration change, decides which of them are
//! notified of which registrations; the delivery itself lives beside it.

use crate::{csource_processor::matching::notified_registration, error::NotificationError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    notification::{CsrChangeEvent, CsrTriggerReason},
    registration::ContextSourceRegistration,
    subscription::SubscriptionStatus,
};
use andromeda_ngsi_ld_query::pagination::{CountPolicy, Pagination};
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{
    change::CsourceSubscriptionChange,
    compilation::compile_csource_subscription,
    compiled::CompiledCsrSubscription,
    csource_cache::CsourceSubscriptionCache,
    csource_matching::matches_csource_subscription,
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use tracing::{debug, error};

mod delivery;
pub(crate) mod matching;

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
    transport: Arc<TransportDispatcher>,
    tenant: TenantRef,
}

impl CsourceNotificationProcessor {
    /// Construct event evaluation over focused policy and delivery capabilities.
    pub fn new(
        target: ContextTarget,
        sub_repo: Arc<dyn SubscriptionStore>,
        csr_repo: Arc<dyn RegistrationStore>,
        transport: Arc<TransportDispatcher>,
        tenant: TenantRef,
    ) -> Self {
        Self {
            target,
            sub_repo,
            csr_repo,
            cache: CsourceSubscriptionCache::new(),
            transport,
            tenant,
        }
    }

    /// Reload compiled context-source subscriptions after startup or an event-stream gap.
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

    /// Send a notification with all currently matching CSRs for a subscription (spec 5.11.3).
    async fn send_matching_csrs_for_subscription(&self, compiled_sub: &CompiledCsrSubscription) {
        let sub_id = &compiled_sub.id;

        // Skip time-interval subscriptions — they have their own periodic worker
        if compiled_sub.time_interval_secs.is_some() {
            return;
        }

        // Fetch all CSRs and filter matching ones
        let mut matching_csrs = Vec::new();
        let page_size = 500;
        let mut offset = 0;

        loop {
            let page = match self
                .csr_repo
                .list(Pagination {
                    limit: page_size,
                    offset,
                    count: CountPolicy::Emit,
                })
                .await
            {
                Ok(page) => page,
                Err(e) => {
                    error!("Failed to fetch CSRs for subscription {} notification: {}", sub_id, e);
                    return;
                }
            };

            if page.registrations.is_empty() {
                break;
            }

            for csr in &page.registrations {
                if matches_csource_subscription(compiled_sub, csr) {
                    match notified_registration(csr, compiled_sub) {
                        Ok(notified) => matching_csrs.push(notified),
                        Err(error) => {
                            error!("Failed to serialize CSR {} for subscription {}: {}", csr.id, sub_id, error);
                            return;
                        }
                    }
                }
            }

            offset += page_size;
            if u64::from(offset) >= page.total {
                break;
            }
        }

        if matching_csrs.is_empty() {
            return;
        }
        self.deliver_csr_notification(compiled_sub, matching_csrs, CsrTriggerReason::NewlyMatching)
            .await;
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

        let csr_data = match csr_data {
            Some(d) => d,
            None => return,
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
        let filtered_csr = match notified_registration(csr_data, compiled_sub) {
            Ok(filtered) => filtered,
            Err(e) => {
                error!("Failed to serialize CSR for subscription {}: {}", compiled_sub.id, e);
                return;
            }
        };
        self.deliver_csr_notification(compiled_sub, vec![filtered_csr], trigger_reason).await;
    }
}
