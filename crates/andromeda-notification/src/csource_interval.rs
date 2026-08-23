//! Periodic context source notifications (clauses 5.11.2.4 and 5.11.7).
//!
//! A subscription with a `timeInterval` is notified of every matching
//! registration on schedule, regardless of changes; the matching is the same
//! clause 5.12 reading the change-driven path uses.

use crate::{csource_processor::matching::notified_registration, error::NotificationError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{dispatch::TransportDispatcher, endpoint::transport::parse_endpoint_transport, request::NotificationDelivery};
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_ngsi_ld_model::{notification::CsourceNotification, subscription::Subscription};
use andromeda_ngsi_ld_query::pagination::{CountPolicy, Pagination};
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{
    compilation::compile_csource_subscription,
    csource_matching::csource_subscription_selects_registration,
    store::SubscriptionStore,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;

/// Bounded evaluation of due context-source interval subscriptions.
pub struct CsourceIntervalNotificationOperation {
    target: ContextTarget,
    sub_repo: Arc<dyn SubscriptionStore>,
    csr_repo: Arc<dyn RegistrationStore>,
    transport: Arc<TransportDispatcher>,
    tenant: TenantRef,
}

impl CsourceIntervalNotificationOperation {
    /// Construct interval evaluation over focused subscription, registration, and delivery capabilities.
    pub const fn new(
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
            transport,
            tenant,
        }
    }

    /// Evaluate every context-source interval subscription due at the execution boundary.
    pub async fn execute(&self) -> Result<usize, NotificationError> {
        let due_subs = self.sub_repo.find_due_interval_subscriptions(&self.target).await?;
        let due_count = due_subs.len();

        for sub in due_subs {
            let csrs = match self.fetch_matching_csrs(&sub).await {
                Ok(c) => c,
                Err(e) => {
                    error!("CsrIntervalWorker failed to fetch CSRs for {}: {}", sub.id, e);
                    if let Err(e) = self.sub_repo.mark_failure(&self.target, &sub.id).await {
                        debug!("Failed to mark failure for {}: {}", sub.id, e);
                    }
                    continue;
                }
            };

            if csrs.is_empty() {
                continue;
            }

            // Claim slot only when we are about to emit a notification.
            let claimed = match self.sub_repo.claim_notification_slot(&self.target, &sub.id, None).await {
                Ok(c) => c,
                Err(e) => {
                    debug!("CsrIntervalWorker failed to claim slot for {}: {}", sub.id, e);
                    continue;
                }
            };

            if !claimed {
                continue;
            }

            let notification_id = match format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Uri>() {
                Ok(u) => u,
                Err(_) => {
                    error!("Failed to construct notification URN for {}", sub.id);
                    continue;
                }
            };
            // clone: the notification owns its subscription id and the
            // subscription is still needed below
            let notification = CsourceNotification::builder()
                .id(notification_id)
                .subscription_id(sub.id.clone())
                .notified_at(Utc::now())
                .data(csrs)
                .build();

            let payload = match serde_json::to_value(&notification) {
                Ok(v) => v,
                Err(e) => {
                    error!("Failed to serialize CsourceNotification for {}: {}", sub.id, e);
                    continue;
                }
            };

            let endpoint_uri = &sub.notification.endpoint.uri;

            let endpoint_transport = match parse_endpoint_transport(endpoint_uri, sub.notification.endpoint.notifier_info.as_deref()) {
                Ok(t) => t,
                Err(e) => {
                    debug!("CsrIntervalWorker: invalid endpoint URI for {}: {}", sub.id, e);
                    continue;
                }
            };

            let request = (NotificationDelivery {
                payload: &payload,
                endpoint_uri: &sub.notification.endpoint.uri,
                accept: sub.notification.endpoint.accept.as_ref(),
                receiver_info: sub.notification.endpoint.receiver_info.as_deref(),
                jsonld_context: sub.delivery.jsonld_context.as_ref(),
                tenant: self.tenant.tenant_name(),
                subscription_id: &sub.id,
            })
            .assemble();

            let success = self.transport.deliver(&endpoint_transport, &request).await.is_delivered();

            if success {
                if let Err(e) = self.sub_repo.mark_success(&self.target, &sub.id).await {
                    debug!("Failed to mark success for {}: {}", sub.id, e);
                }
            } else {
                if let Err(e) = self.sub_repo.mark_failure(&self.target, &sub.id).await {
                    debug!("Failed to mark failure for {}: {}", sub.id, e);
                }
            }
        }
        Ok(due_count)
    }

    async fn fetch_matching_csrs(&self, sub: &Subscription) -> Result<Vec<Value>, NotificationError> {
        let compiled = compile_csource_subscription(sub);
        let mut matching = Vec::new();
        let page_size = 500;
        let mut offset = 0;

        loop {
            let page = self
                .csr_repo
                .list(Pagination {
                    limit: page_size,
                    offset,
                    count: CountPolicy::Emit,
                })
                .await?;
            if page.registrations.is_empty() {
                break;
            }

            for csr in &page.registrations {
                if csource_subscription_selects_registration(&compiled, csr) {
                    matching.push(notified_registration(csr, &compiled)?);
                }
            }

            offset += page_size;
            if u64::from(offset) >= page.total {
                break;
            }
        }

        Ok(matching)
    }
}
