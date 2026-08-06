use crate::{
    repository::{csource_subscription::repo::CsourceSubscriptionRepository, subscription::repo::SubscriptionRepository},
    service::subscription::store::SubscriptionStore,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::{CsourceSubscriptionChange, SubscriptionChange};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info};

/// Combined worker that expires both entity subscriptions and CSR subscriptions.
pub struct SubscriptionExpirationWorker {
    subscription_repo: SubscriptionRepository,
    subscription_change_tx: broadcast::Sender<SubscriptionChange>,
    csource_subscription_repo: CsourceSubscriptionRepository,
    csource_subscription_change_tx: broadcast::Sender<CsourceSubscriptionChange>,
    interval: Duration,
}

impl SubscriptionExpirationWorker {
    pub const fn new(
        subscription_repo: SubscriptionRepository,
        subscription_change_tx: broadcast::Sender<SubscriptionChange>,
        csource_subscription_repo: CsourceSubscriptionRepository,
        csource_subscription_change_tx: broadcast::Sender<CsourceSubscriptionChange>,
        interval_secs: u64,
    ) -> SubscriptionExpirationWorker {
        SubscriptionExpirationWorker {
            subscription_repo,
            subscription_change_tx,
            csource_subscription_repo,
            csource_subscription_change_tx,
            interval: Duration::from_secs(interval_secs),
        }
    }

    pub async fn run(self) {
        info!("SubscriptionExpirationWorker starting (interval: {:?})", self.interval);
        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            match self.subscription_repo.expire_due().await {
                Ok(expired_ids) => {
                    if !expired_ids.is_empty() {
                        info!("Expired {} subscription(s)", expired_ids.len());
                        for id in expired_ids {
                            match id.parse::<Uri>() {
                                Ok(uri) => {
                                    let _ = self.subscription_change_tx.send(SubscriptionChange::Deleted(uri));
                                }
                                Err(_) => {
                                    error!("Failed to parse expired subscription ID as URI: {}", id);
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    error!("SubscriptionExpirationWorker failed to expire subscriptions: {err}");
                }
            }

            match self.csource_subscription_repo.expire_due().await {
                Ok(expired_ids) => {
                    if !expired_ids.is_empty() {
                        info!("Expired {} CSR subscription(s)", expired_ids.len());
                        for id in expired_ids {
                            match id.parse::<Uri>() {
                                Ok(uri) => {
                                    let _ = self.csource_subscription_change_tx.send(CsourceSubscriptionChange::Deleted(uri));
                                }
                                Err(_) => {
                                    error!("Failed to parse expired CSR subscription ID as URI: {}", id);
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    error!("SubscriptionExpirationWorker failed to expire CSR subscriptions: {err}");
                }
            }
        }
    }
}
