use crate::{
    domain::notification::{ChangeEvent, TriggerReason},
    repository::entity::EntityRepository,
};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{error, info};

/// Combined worker that expires both entities and attribute instances.
pub struct DataExpirationWorker {
    entity_repo: EntityRepository,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    interval: Duration,
}

impl DataExpirationWorker {
    pub const fn new(entity_repo: EntityRepository, change_tx: Option<broadcast::Sender<ChangeEvent>>, interval_secs: u64) -> DataExpirationWorker {
        DataExpirationWorker {
            entity_repo,
            change_tx,
            interval: Duration::from_secs(interval_secs),
        }
    }

    pub async fn run(self) {
        info!("DataExpirationWorker starting (interval: {:?})", self.interval);
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
                Err(err) => {
                    error!("DataExpirationWorker failed to expire entities: {err}");
                }
            }

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
                Err(err) => {
                    error!("DataExpirationWorker failed to expire attributes: {err}");
                }
            }
        }
    }
}
