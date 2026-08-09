use crate::{
    event::{csr_change_event::CsrChangeEvent, subscription_change::CsourceSubscriptionChange},
    service::{
        compiled_subscription::CompiledCsrSubscription,
        csource_registration::store::RegistrationStore,
        subscription::store::SubscriptionStore,
        subscription_throttle::ceiling_for_throttle,
    },
    worker::csr_notification::matching::{compile_csr_subscription, filter_csr_for_subscription, find_matching_csr_subscriptions, matches_csr_subscription},
};
use andromeda_delivery::{dispatch::TransportDispatcher, endpoint::transport::parse_endpoint_transport, request::NotificationDelivery};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    uri::{NGSI_LD_URN_NID, Uri},
};
use andromeda_ngsi_ld_model::{
    notification::{CsourceNotification, CsourceNotificationType, CsrTriggerReason},
    subscription::{EntitySelector, Subscription, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::filter::eval;
use andromeda_postgresql_context_store::csource_subscription::repo::CsourceSubscriptionRepository;
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use dashmap::DashMap;
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::{broadcast, broadcast::error::TryRecvError};
use tracing::{debug, error, info};
use uuid::Uuid;

mod matching;

const CSR_NOTIFICATION_CONCURRENCY: usize = 20;

pub struct CsrNotificationWorker {
    csr_change_rx: broadcast::Receiver<CsrChangeEvent>,
    sub_change_rx: broadcast::Receiver<CsourceSubscriptionChange>,
    sub_repo: CsourceSubscriptionRepository,
    csr_repo: Arc<dyn RegistrationStore>,
    cache: DashMap<Uri, Arc<CompiledCsrSubscription>>,
    transport: Arc<TransportDispatcher>,
    tenant: TenantRef,
}

impl CsrNotificationWorker {
    pub fn new(
        csr_change_rx: broadcast::Receiver<CsrChangeEvent>,
        sub_change_rx: broadcast::Receiver<CsourceSubscriptionChange>,
        sub_repo: CsourceSubscriptionRepository,
        csr_repo: Arc<dyn RegistrationStore>,
        transport: Arc<TransportDispatcher>,
        tenant: TenantRef,
    ) -> Self {
        Self {
            csr_change_rx,
            sub_change_rx,
            sub_repo,
            csr_repo,
            cache: DashMap::new(),
            transport,
            tenant,
        }
    }

    /// The delivery one CSR notification becomes.
    ///
    /// Three call sites send the same notification for three different
    /// reasons -- a subscription that just started matching, a registration
    /// that changed, and a registration that stopped matching -- and every one
    /// of them addresses the endpoint identically.
    fn delivery_for<'a>(&'a self, compiled_sub: &'a CompiledCsrSubscription, payload: &'a Value, sub_id: &'a Uri) -> NotificationDelivery<'a> {
        NotificationDelivery {
            payload,
            endpoint_uri: &compiled_sub.endpoint_uri,
            accept: compiled_sub.endpoint_accept.as_ref(),
            receiver_info: compiled_sub.endpoint_receiver_info.as_deref(),
            jsonld_context: compiled_sub.jsonld_context.as_ref(),
            tenant: self.tenant.tenant_name(),
            subscription_id: sub_id,
        }
    }

    pub async fn run(mut self) {
        info!("CsrNotificationWorker starting, loading active CSR subscriptions...");

        if let Err(e) = self.reload_cache().await {
            error!("Failed to load CSR subscriptions into cache: {}", e);
        }

        info!("CsrNotificationWorker loaded {} active CSR subscriptions", self.cache.len());

        loop {
            tokio::select! {
                result = self.csr_change_rx.recv() => {
                    match result {
                        Ok(event) => {
                            self.drain_subscription_changes().await;
                            self.handle_csr_change(event).await;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            debug!("CsrNotificationWorker lagged, missed {} CSR events", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("CSR change channel closed, shutting down CsrNotificationWorker");
                            return;
                        }
                    }
                }
                result = self.sub_change_rx.recv() => {
                    match result {
                        Ok(change) => {
                            self.handle_subscription_change(change).await;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            debug!("CsrNotificationWorker lagged on subscription changes, missed {}", n);
                            if let Err(e) = self.reload_cache().await {
                                error!("Failed to reload CSR subscription cache: {}", e);
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("CSR subscription change channel closed, shutting down CsrNotificationWorker");
                            return;
                        }
                    }
                }
            }
        }
    }

    async fn reload_cache(&self) -> Result<(), crate::error::AndromedaError> {
        let subscriptions = self.sub_repo.list_active().await?;
        self.cache.clear();
        for sub in subscriptions {
            if let Some(compiled) = compile_csr_subscription(&sub) {
                self.cache.insert(sub.id.clone(), Arc::new(compiled)); // clone: the map owns its key
            }
        }
        Ok(())
    }

    /// Apply every already-published subscription change before matching a CSR.
    ///
    /// Subscription and registration mutations use separate broadcast channels.
    /// A later registration event can therefore win `select!` while an earlier
    /// subscription pause, expiration or selector update is still queued. The
    /// database mutation is already committed when its event is published, so
    /// draining that queue restores the request order at the matching seam.
    async fn drain_subscription_changes(&mut self) {
        loop {
            match self.sub_change_rx.try_recv() {
                Ok(change) => self.handle_subscription_change(change).await,
                Err(TryRecvError::Empty | TryRecvError::Closed) => return,
                Err(TryRecvError::Lagged(skipped)) => {
                    debug!(skipped, "CsrNotificationWorker lagged while ordering subscription changes before a CSR event");
                    if let Err(error) = self.reload_cache().await {
                        error!(%error, "Failed to reload CSR subscription cache after lag");
                    }
                }
            }
        }
    }

    async fn handle_subscription_change(&self, change: CsourceSubscriptionChange) {
        match change {
            CsourceSubscriptionChange::Upserted(id) => {
                match self.sub_repo.get(&id).await {
                    Ok(Some(sub)) => {
                        let compiled = if sub.lifecycle.status == SubscriptionStatus::Active {
                            compile_csr_subscription(&sub).map(Arc::new)
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
            CsourceSubscriptionChange::Deleted(id) => {
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
        let page_size: i64 = 500;
        let mut offset: i64 = 0;

        loop {
            let (page, total) = match self.csr_repo.list(Some(page_size), Some(offset)).await {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to fetch CSRs for subscription {} notification: {}", sub_id, e);
                    return;
                }
            };

            if page.is_empty() {
                break;
            }

            for csr in page {
                let csr_value = match serde_json::to_value(&csr) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if matches_csr_subscription(compiled_sub, &csr_value) {
                    let filtered = filter_csr_for_subscription(&csr_value, compiled_sub);
                    matching_csrs.push(filtered);
                }
            }

            offset += page_size;
            if offset >= total {
                break;
            }
        }

        if matching_csrs.is_empty() {
            return;
        }

        match self
            .sub_repo
            .claim_notification_slot(sub_id, ceiling_for_throttle(Utc::now(), compiled_sub.throttling_secs))
            .await
        {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                debug!("Failed to claim notification slot for {}: {}", sub_id, e);
                return;
            }
        }

        let notification_id = match format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Uri>() {
            Ok(u) => u,
            Err(_) => {
                error!("Failed to construct notification URN for {}", sub_id);
                return;
            }
        };
        // clone: the builder takes the id by value and `sub_id` is still needed for delivery
        let sub_uri = sub_id.clone();
        let notification = CsourceNotification::builder()
            .id(notification_id)
            .type_(CsourceNotificationType)
            .subscription_id(sub_uri)
            .notified_at(Utc::now())
            .data(matching_csrs)
            .trigger_reason(Some(CsrTriggerReason::NewlyMatching))
            .build();

        let payload = match serde_json::to_value(&notification) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to serialize CsourceNotification for {}: {}", sub_id, e);
                return;
            }
        };

        let request = self.delivery_for(compiled_sub, &payload, sub_id).assemble();
        let success = self.transport.deliver(&compiled_sub.endpoint_transport, &request).await.is_delivered();

        if success {
            if let Err(e) = self.sub_repo.mark_success(sub_id).await {
                debug!("Failed to mark CSR notification success for {}: {}", sub_id, e);
            }
        } else {
            if let Err(e) = self.sub_repo.mark_failure(sub_id).await {
                debug!("Failed to mark CSR notification failure for {}: {}", sub_id, e);
            }
        }
    }

    async fn handle_csr_change(&self, event: CsrChangeEvent) {
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
            CsrTriggerReason::NoLongerMatching => event.csr_snapshot.clone(),
            _ => match self.csr_repo.get(&event.csr_id).await {
                Ok(Some(csr)) => serde_json::to_value(&csr).ok(),
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

        let matching = find_matching_csr_subscriptions(&self.cache, &csr_data);

        if matching.is_empty() {
            return;
        }

        let trigger_reason = event.trigger_reason;
        let _results: Vec<()> = stream::iter(matching)
            .map(|compiled_sub| {
                let csr_data = csr_data.clone();
                async move {
                    let sub_id = compiled_sub.id.clone();

                    match self
                        .sub_repo
                        .claim_notification_slot(&sub_id, ceiling_for_throttle(Utc::now(), compiled_sub.throttling_secs))
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(e) => {
                            debug!("Failed to claim CSR notification slot for {}: {}", sub_id, e);
                            return;
                        }
                    }

                    // Build notification with the CSR data, filtering information to matching entries
                    let filtered_csr = filter_csr_for_subscription(&csr_data, &compiled_sub);

                    let notification_id = match format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Uri>() {
                        Ok(u) => u,
                        Err(_) => {
                            error!("Failed to construct notification URN for {}", sub_id);
                            return;
                        }
                    };
                    // clone: the builder takes the id by value and `sub_id` is still needed for delivery
                    let sub_uri = sub_id.clone();
                    let notification = CsourceNotification::builder()
                        .id(notification_id)
                        .type_(CsourceNotificationType)
                        .subscription_id(sub_uri)
                        .notified_at(Utc::now())
                        .data(vec![filtered_csr])
                        .trigger_reason(Some(trigger_reason))
                        .build();

                    let payload = match serde_json::to_value(&notification) {
                        Ok(v) => v,
                        Err(e) => {
                            error!("Failed to serialize CsourceNotification for {}: {}", sub_id, e);
                            return;
                        }
                    };

                    let request = self.delivery_for(&compiled_sub, &payload, &sub_id).assemble();
                    let success = self.transport.deliver(&compiled_sub.endpoint_transport, &request).await.is_delivered();

                    if success {
                        if let Err(e) = self.sub_repo.mark_success(&sub_id).await {
                            debug!("Failed to mark CSR notification success for {}: {}", sub_id, e);
                        }
                    } else if let Err(e) = self.sub_repo.mark_failure(&sub_id).await {
                        debug!("Failed to mark CSR notification failure for {}: {}", sub_id, e);
                    }
                }
            })
            .buffer_unordered(CSR_NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
    }

    /// Handle a CSR update where we have a pre-update snapshot.
    /// Sends "updated" to subscriptions that still match, and "noLongerMatching"
    /// to subscriptions that matched the old CSR but not the new one.
    async fn handle_csr_update_with_snapshot(&self, event: &CsrChangeEvent, pre_update: &Value) {
        // Fetch post-update CSR from DB
        let post_update = match self.csr_repo.get(&event.csr_id).await {
            Ok(Some(csr)) => match serde_json::to_value(&csr) {
                Ok(v) => v,
                Err(_) => return,
            },
            Ok(None) => return,
            Err(e) => {
                error!("Failed to fetch CSR {} for update notification: {}", event.csr_id, e);
                return;
            }
        };

        let matched_before = find_matching_csr_subscriptions(&self.cache, pre_update);
        let matched_after = find_matching_csr_subscriptions(&self.cache, &post_update);

        let after_ids: HashSet<&Uri> = matched_after.iter().map(|subscription| &subscription.id).collect();
        let before_ids: HashSet<&Uri> = matched_before.iter().map(|subscription| &subscription.id).collect();

        // Subscriptions that still match → "updated"
        let updated_subs: Vec<Arc<CompiledCsrSubscription>> = matched_after.iter().filter(|sub| before_ids.contains(&sub.id)).cloned().collect();
        let _updated: Vec<()> = stream::iter(updated_subs)
            .map(|sub| {
                let post_update = post_update.clone();
                async move {
                    self.send_csr_notification(&sub, &post_update, CsrTriggerReason::Updated).await;
                }
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
            .map(|sub| {
                let post_update = post_update.clone();
                async move {
                    self.send_csr_notification(&sub, &post_update, CsrTriggerReason::NewlyMatching).await;
                }
            })
            .buffer_unordered(CSR_NOTIFICATION_CONCURRENCY)
            .collect()
            .await;
    }

    async fn send_csr_notification(&self, compiled_sub: &CompiledCsrSubscription, csr_data: &Value, trigger_reason: CsrTriggerReason) {
        let sub_id = &compiled_sub.id;

        match self
            .sub_repo
            .claim_notification_slot(sub_id, ceiling_for_throttle(Utc::now(), compiled_sub.throttling_secs))
            .await
        {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                debug!("Failed to claim CSR notification slot for {}: {}", sub_id, e);
                return;
            }
        }

        let filtered_csr = filter_csr_for_subscription(csr_data, compiled_sub);

        let notification_id = match format!("urn:{}:Notification:{}", NGSI_LD_URN_NID, Uuid::new_v4()).parse::<Uri>() {
            Ok(u) => u,
            Err(_) => {
                error!("Failed to construct notification URN for {}", sub_id);
                return;
            }
        };
        // clone: the builder takes the id by value and `sub_id` is still needed for delivery
        let sub_uri = sub_id.clone();
        let notification = CsourceNotification::builder()
            .id(notification_id)
            .type_(CsourceNotificationType)
            .subscription_id(sub_uri)
            .notified_at(Utc::now())
            .data(vec![filtered_csr])
            .trigger_reason(Some(trigger_reason))
            .build();

        let payload = match serde_json::to_value(&notification) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to serialize CsourceNotification for {}: {}", sub_id, e);
                return;
            }
        };

        let request = self.delivery_for(compiled_sub, &payload, sub_id).assemble();
        let success = self.transport.deliver(&compiled_sub.endpoint_transport, &request).await.is_delivered();

        if success {
            if let Err(e) = self.sub_repo.mark_success(sub_id).await {
                debug!("Failed to mark CSR notification success for {}: {}", sub_id, e);
            }
        } else {
            if let Err(e) = self.sub_repo.mark_failure(sub_id).await {
                debug!("Failed to mark CSR notification failure for {}: {}", sub_id, e);
            }
        }
    }
}
