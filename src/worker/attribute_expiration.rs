//! Background worker that periodically removes expired attribute instances from entity payloads (clause 4.22).

use crate::{
    domain::notification::{ChangeEvent, TriggerReason},
    repository::entity::EntityRepository,
};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info};

/// Periodically scans for attribute instances whose `expiresAt` has passed
/// and removes them from the entity JSONB payload.
pub struct AttributeExpirationWorker {
    entity_repo: EntityRepository,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    interval: Duration,
}

#[async_trait::async_trait]
impl super::Worker for AttributeExpirationWorker {
    async fn run(self) {
        AttributeExpirationWorker::run(self).await
    }
}

impl AttributeExpirationWorker {
    /// Create a new attribute expiration worker.
    pub fn new(entity_repo: EntityRepository, change_tx: Option<broadcast::Sender<ChangeEvent>>, interval_secs: u64) -> AttributeExpirationWorker {
        AttributeExpirationWorker {
            entity_repo,
            change_tx,
            interval: Duration::from_secs(interval_secs),
        }
    }

    /// Run the worker loop, periodically removing expired attribute instances.
    pub async fn run(self) {
        info!("AttributeExpirationWorker starting (interval: {:?})", self.interval);

        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            match self.entity_repo.expire_due_attributes().await {
                Ok(affected_ids) => {
                    if !affected_ids.is_empty() {
                        info!("Removed expired attribute instances from {} entity/entities", affected_ids.len());
                        if let Some(ref tx) = self.change_tx {
                            for id in affected_ids {
                                let _ = tx.send(ChangeEvent::new(id, Vec::new(), TriggerReason::EntityUpdated));
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("AttributeExpirationWorker failed: {}", e);
                }
            }
        }
    }
}
