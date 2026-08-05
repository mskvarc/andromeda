use crate::{
    repository::{
        csource_registration::repo::CsourceRegistrationRepository,
        csource_subscription::repo::CsourceSubscriptionRepository,
        subscription_store::repo::SubscriptionStore,
    },
    transport::{
        dispatch::{DeliveryResult, TransportDispatcher, build_delivery_request},
        endpoint::parse_endpoint_transport,
    },
};
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_ngsi_ld_model::{notification::CsourceNotification, subscription::Subscription};
use chrono::Utc;
use mediatype::MediaTypeBuf;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, info};
use url::Url;
use uuid::Uuid;

pub struct CsrIntervalWorker {
    sub_repo: CsourceSubscriptionRepository,
    csr_repo: CsourceRegistrationRepository,
    transport: Arc<TransportDispatcher>,
    poll_interval: Duration,
    tenant: crate::tenancy::types::TenantRef,
}

impl CsrIntervalWorker {
    pub const fn new(
        sub_repo: CsourceSubscriptionRepository,
        csr_repo: CsourceRegistrationRepository,
        transport: Arc<TransportDispatcher>,
        tenant: crate::tenancy::types::TenantRef,
    ) -> Self {
        Self {
            sub_repo,
            csr_repo,
            transport,
            poll_interval: Duration::from_secs(1),
            tenant,
        }
    }

    pub async fn run(self) {
        info!("CsrIntervalWorker starting (poll interval: {:?})", self.poll_interval);

        let mut ticker = tokio::time::interval(self.poll_interval);

        loop {
            ticker.tick().await;

            let due_subs = match self.sub_repo.find_due_interval_subscriptions().await {
                Ok(subs) => subs,
                Err(e) => {
                    error!("CsrIntervalWorker failed to find due subscriptions: {}", e);
                    continue;
                }
            };

            for sub in due_subs {
                let csrs = match self.fetch_matching_csrs(&sub).await {
                    Ok(c) => c,
                    Err(e) => {
                        error!("CsrIntervalWorker failed to fetch CSRs for {}: {}", sub.id, e);
                        if let Err(e) = self.sub_repo.mark_failure(sub.id.as_ref()).await {
                            debug!("Failed to mark failure for {}: {}", sub.id, e);
                        }
                        continue;
                    }
                };

                if csrs.is_empty() {
                    continue;
                }

                // Claim slot only when we are about to emit a notification.
                let claimed = match self.sub_repo.claim_notification_slot(sub.id.as_ref(), None).await {
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

                let endpoint_uri = sub.notification.endpoint.uri.as_str();
                let accept = sub.notification.endpoint.accept.as_ref().map(MediaTypeBuf::as_str);
                let jsonld_context = sub.delivery.jsonld_context.as_ref().map(Url::as_str);

                let endpoint_transport = match parse_endpoint_transport(endpoint_uri, sub.notification.endpoint.notifier_info.as_deref()) {
                    Ok(t) => t,
                    Err(e) => {
                        debug!("CsrIntervalWorker: invalid endpoint URI for {}: {}", sub.id, e);
                        continue;
                    }
                };

                let request = match build_delivery_request(
                    &payload,
                    endpoint_uri,
                    accept,
                    sub.notification.endpoint.receiver_info.as_deref(),
                    jsonld_context,
                    self.tenant.tenant_name(),
                    sub.id.as_ref(),
                ) {
                    Ok(r) => r,
                    Err(e) => {
                        debug!("CsrIntervalWorker: failed to build delivery request for {}: {}", sub.id, e);
                        continue;
                    }
                };

                let success = matches!(self.transport.deliver(&endpoint_transport, &request).await, DeliveryResult::Delivered);

                if success {
                    if let Err(e) = self.sub_repo.mark_success(sub.id.as_ref()).await {
                        debug!("Failed to mark success for {}: {}", sub.id, e);
                    }
                } else {
                    if let Err(e) = self.sub_repo.mark_failure(sub.id.as_ref()).await {
                        debug!("Failed to mark failure for {}: {}", sub.id, e);
                    }
                }
            }
        }
    }

    async fn fetch_matching_csrs(&self, sub: &Subscription) -> Result<Vec<Value>, crate::core::error::AndromedaError> {
        let mut matching = Vec::new();
        let page_size: i64 = 500;
        let mut offset: i64 = 0;

        loop {
            let (page, total) = self.csr_repo.list(Some(page_size), Some(offset)).await?;
            if page.is_empty() {
                break;
            }

            for csr in page {
                let csr_value =
                    serde_json::to_value(&csr).map_err(|e| crate::core::error::AndromedaError::internal(format!("Failed to serialize CSR: {}", e)))?;
                if matches_csr_for_subscription(&csr_value, sub) {
                    matching.push(csr_value);
                }
            }

            offset += page_size;
            if offset >= total {
                break;
            }
        }

        Ok(matching)
    }
}

fn matches_csr_for_subscription(csr_data: &Value, sub: &Subscription) -> bool {
    // Entity selector matching
    if let Some(ref selectors) = sub.selection.entities {
        let information = csr_data.get("information").and_then(|v| v.as_array());
        let any_matches = selectors.iter().any(|sel| match information {
            Some(info_arr) => info_arr.iter().any(|info| {
                let entities = info.get("entities").and_then(|v| v.as_array());
                match entities {
                    Some(ents) => ents.iter().any(|ent| {
                        let ent_type = ent.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if ent_type != sel.type_ {
                            return false;
                        }
                        if let Some(ref id) = sel.id {
                            let ent_id = ent.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            if ent_id != id {
                                return false;
                            }
                        }
                        if let Some(ref pattern) = sel.id_pattern
                            && let Ok(re) = regex::Regex::new(pattern)
                        {
                            let ent_id = ent.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            if !re.is_match(ent_id) {
                                return false;
                            }
                        }
                        true
                    }),
                    None => false,
                }
            }),
            None => false,
        });

        if !any_matches {
            return false;
        }
    }

    // Watched attributes matching
    if let Some(ref watched) = sub.selection.watched_attributes {
        let watched_set: std::collections::HashSet<&str> = watched.iter().map(|s| s.as_str()).collect();
        let information = csr_data.get("information").and_then(|v| v.as_array());
        let any_attr_matches = match information {
            Some(info_arr) => info_arr.iter().any(|info| {
                let prop_names: Vec<&str> = info
                    .get("propertyNames")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let rel_names: Vec<&str> = info
                    .get("relationshipNames")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                prop_names.iter().any(|n| watched_set.contains(n)) || rel_names.iter().any(|n| watched_set.contains(n))
            }),
            None => false,
        };

        if !any_attr_matches {
            return false;
        }
    }

    true
}
