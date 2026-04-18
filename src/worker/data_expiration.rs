use crate::{
    core::batch_activity::BatchActivityTracker,
    domain::{
        notification::{ChangeEvent, TriggerReason},
        uri::Uri,
    },
    repository::entity::repo::EntityRepository,
};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Combined worker that expires both entities and attribute instances.
pub struct DataExpirationWorker {
    entity_repo: EntityRepository,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    interval: Duration,
    batch_activity: BatchActivityTracker,
}

impl DataExpirationWorker {
    pub const fn new(
        entity_repo: EntityRepository,
        change_tx: Option<broadcast::Sender<ChangeEvent>>,
        interval_secs: u64,
        batch_activity: BatchActivityTracker,
    ) -> DataExpirationWorker {
        DataExpirationWorker {
            entity_repo,
            change_tx,
            interval: Duration::from_secs(interval_secs),
            batch_activity,
        }
    }

    pub async fn run(self) {
        info!("DataExpirationWorker starting (interval: {:?})", self.interval);
        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            if self.batch_activity.is_within_cooldown() {
                debug!("DataExpirationWorker skipping scan: batch activity within cooldown");
                continue;
            }

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
                Err(err) => {
                    error!("DataExpirationWorker failed to expire entities: {err}");
                }
            }

            match self.entity_repo.purge_expired_attributes().await {
                Ok(affected_ids) => {
                    if !affected_ids.is_empty() {
                        info!("Removed expired attribute instances from {} entity/entities", affected_ids.len());
                        if let Some(ref tx) = self.change_tx {
                            for id in affected_ids {
                                match id.parse::<Uri>() {
                                    Ok(entity_uri) => {
                                        let _ = tx.send(ChangeEvent::new(entity_uri, Vec::new(), TriggerReason::EntityUpdated));
                                    }
                                    Err(_) => {
                                        error!(entity_id = %id, event = "entity_uri_parse_failed");
                                    }
                                }
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
