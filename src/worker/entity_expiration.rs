//! Background worker that periodically deletes entities whose expiresAt has passed (§4.22).

use crate::{
    service::entity::store::EntityStore,
};
use andromeda_ngsi_ld_model::notification::ChangeEvent;
use andromeda_ngsi_ld_model::notification::TriggerReason;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info};

pub struct EntityExpirationWorker {
    entity_repo: Arc<dyn EntityStore>,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    interval: Duration,
}

#[async_trait::async_trait]
impl super::Worker for EntityExpirationWorker {
    async fn run(self) {
        EntityExpirationWorker::run(self).await
    }
}

impl EntityExpirationWorker {
    pub const fn new(entity_repo: Arc<dyn EntityStore>, change_tx: Option<broadcast::Sender<ChangeEvent>>, interval_secs: u64) -> Self {
        Self {
            entity_repo,
            change_tx,
            interval: Duration::from_secs(interval_secs),
        }
    }

    pub async fn run(self) {
        info!("EntityExpirationWorker starting (interval: {:?})", self.interval);

        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            match self.entity_repo.expire_due_entities().await {
                Ok(expired_ids) => {
                    if !expired_ids.is_empty() {
                        info!("Expired {} entity/entities", expired_ids.len());
                        if let Some(ref tx) = self.change_tx {
                            for id in expired_ids {
                                match id.parse::<Uri>() {
                                    Ok(entity_uri) => {
                                        let _ = tx.send(ChangeEvent::new(entity_uri, Vec::new(), TriggerReason::EntityDeleted));
                                    }
                                    Err(_) => {
                                        error!(entity_id = %id, event = "entity_uri_parse_failed");
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("EntityExpirationWorker failed to expire entities: {}", e);
                }
            }
        }
    }
}
