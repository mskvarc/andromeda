use crate::repository::subscription_store::repo::SubscriptionStore;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info};

/// Generic expiration worker parameterized over store type and change event type.
///
/// Replaces both `ExpirationWorker` (entity subscriptions) and
/// `CsrExpirationWorker` (CSR subscriptions) with a single implementation.
pub struct ExpirationWorker<S: SubscriptionStore, C: Clone + Send + 'static> {
    sub_repo: S,
    sub_change_tx: broadcast::Sender<C>,
    interval: Duration,
    to_change_deleted: fn(String) -> C,
    label: &'static str,
}

#[async_trait::async_trait]
impl<S: SubscriptionStore + Send + 'static, C: Clone + Send + 'static> super::Worker for ExpirationWorker<S, C> {
    async fn run(self) {
        ExpirationWorker::run(self).await
    }
}

impl<S: SubscriptionStore, C: Clone + Send + 'static> ExpirationWorker<S, C> {
    pub fn new(sub_repo: S, sub_change_tx: broadcast::Sender<C>, to_change_deleted: fn(String) -> C, label: &'static str, interval_secs: u64) -> Self {
        Self {
            sub_repo,
            sub_change_tx,
            interval: Duration::from_secs(interval_secs),
            to_change_deleted,
            label,
        }
    }

    pub async fn run(self) {
        info!("{}ExpirationWorker starting (interval: {:?})", self.label, self.interval);

        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            match self.sub_repo.expire_due().await {
                Ok(expired_ids) => {
                    if !expired_ids.is_empty() {
                        info!("Expired {} {}subscription(s)", expired_ids.len(), self.label);
                        for id in expired_ids {
                            let _ = self.sub_change_tx.send((self.to_change_deleted)(id));
                        }
                    }
                }
                Err(e) => {
                    error!("{}ExpirationWorker failed to expire subscriptions: {}", self.label, e);
                }
            }
        }
    }
}
