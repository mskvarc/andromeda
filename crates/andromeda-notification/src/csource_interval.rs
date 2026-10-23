//! Periodic context source notifications (ETSI GS CIM 009 clauses 5.11.2.4 and
//! 5.11.7).
//!
//! A subscription carrying a `timeInterval` is notified "periodically,
//! initially on subscription and when the time interval (in seconds) specified
//! in such value field is reached, independent of any changes to the set of
//! Context Source registrations". The initial notification is sent as the
//! subscription is created, by the same path that answers clause 5.11.3.4's
//! update; what is left here is the elapsed interval, and the matching, the
//! trimming and the sending are the ones the change-driven path uses.

use crate::{csource_delivery::CsourceNotificationDelivery, csource_match_set::matching_registrations, error::NotificationError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, registration::ContextSourceRegistration};
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{compilation::compile_csource_subscription, store::SubscriptionStore};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::error;

/// Bounded evaluation of due context-source interval subscriptions.
pub struct CsourceIntervalNotificationOperation {
    /// The tenant branch the due subscriptions are read from.
    target: ContextTarget,
    /// The store the due subscriptions are read from.
    sub_repo: Arc<dyn SubscriptionStore>,
    /// The store the matching registrations are read from.
    csr_repo: Arc<dyn RegistrationStore>,
    /// The delivery half every context source notification goes through.
    delivery: CsourceNotificationDelivery,
}

impl CsourceIntervalNotificationOperation {
    /// Construct interval evaluation over focused subscription, registration, and delivery capabilities.
    pub fn new(
        target: ContextTarget,
        sub_repo: Arc<dyn SubscriptionStore>,
        csr_repo: Arc<dyn RegistrationStore>,
        engine: JsonLdEngine,
        transport: Arc<TransportDispatcher>,
        tenant: TenantRef,
    ) -> CsourceIntervalNotificationOperation {
        CsourceIntervalNotificationOperation {
            target: target.clone(), // clone: the operation and its delivery half address the same branch.
            sub_repo: Arc::clone(&sub_repo),
            csr_repo,
            delivery: CsourceNotificationDelivery::new(target, sub_repo, engine, transport, tenant),
        }
    }

    /// Evaluate every context-source interval subscription due at the execution boundary.
    ///
    /// Clause 5.11.7 makes the periodic notification independent of any change
    /// to the set of registrations, so an interval that elapses over an empty
    /// match set still sends -- with an empty `data`, which is the current
    /// answer to what the subscription asked.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Subscription`] when the subscription store cannot
    /// report the due interval subscriptions. Per-subscription failures -- registration
    /// lookup and serialization -- are logged and skipped rather than aborting the sweep.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<usize, NotificationError> {
        let due_subs = self.sub_repo.find_due_interval_subscriptions(&self.target, cutoff).await?;
        let due_count = due_subs.len();

        for stored in due_subs {
            let compiled = compile_csource_subscription(&stored.subscription);
            let matching = match matching_registrations(self.csr_repo.as_ref(), &compiled).await {
                Ok(matching) => matching,
                Err(error) => {
                    error!("Failed to fetch CSRs for interval subscription {}: {}", compiled.id, error);
                    continue;
                }
            };
            let notified: Vec<&ContextSourceRegistration> = matching.iter().collect();
            let data = match self.delivery.notified_registrations(&notified, &compiled).await {
                Ok(data) => data,
                Err(error) => {
                    error!("Failed to serialize CSRs for interval subscription {}: {}", compiled.id, error);
                    continue;
                }
            };
            // Clause 5.3.3: `newlyMatching` describes the registrations that
            // match the subscription now, which is what a periodic
            // notification reports whether or not the set has changed.
            self.delivery.deliver(&compiled, data, CsrTriggerReason::NewlyMatching).await;
        }
        Ok(due_count)
    }

    /// The earliest instant some active context-source interval subscription is
    /// next due after `cutoff`, or `None` when this tenant has none.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Subscription`] when the store cannot report
    /// the next due instant.
    pub async fn next_due(&self, cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, NotificationError> {
        self.sub_repo.next_interval_due(&self.target, cutoff).await.map_err(NotificationError::from)
    }
}
