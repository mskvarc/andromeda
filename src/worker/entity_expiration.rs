//! Background worker that periodically deletes entities whose expiresAt has passed (§4.22).

use crate::repository::entity::EntityRepository;
use ngsi_ld_types::notification::{ChangeEvent, TriggerReason};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info};

pub struct EntityExpirationWorker {
    entity_repo: EntityRepository,
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
    pub fn new(entity_repo: EntityRepository, change_tx: Option<broadcast::Sender<ChangeEvent>>, interval_secs: u64) -> Self {
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
                                let _ = tx.send(ChangeEvent::new(id, Vec::new(), TriggerReason::EntityDeleted));
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
