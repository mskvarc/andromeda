use crate::{
    core::{batch_activity::BatchActivityTracker, error::AndromedaError},
    repository::{entity::repo::EntityRepository, uow::UnitOfWorkFactory},
};
use andromeda_domain::{
    expanded::instance_key::removed_attribute_instances,
    notification::{ChangeEvent, TriggerReason},
    uri::Uri,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Combined worker that expires both entities and attribute instances.
pub struct DataExpirationWorker {
    entity_repo: EntityRepository,
    uow: Arc<UnitOfWorkFactory>,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    interval: Duration,
    batch_activity: BatchActivityTracker,
}

impl DataExpirationWorker {
    pub const fn new(
        entity_repo: EntityRepository,
        uow: Arc<UnitOfWorkFactory>,
        change_tx: Option<broadcast::Sender<ChangeEvent>>,
        interval_secs: u64,
        batch_activity: BatchActivityTracker,
    ) -> DataExpirationWorker {
        DataExpirationWorker {
            entity_repo,
            uow,
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

            if let Err(err) = self.purge_expired_attributes().await {
                error!("DataExpirationWorker failed to expire attributes: {err}");
            }
        }
    }

    /// Remove past-due attribute instances and record a tombstone for each.
    ///
    /// An expiring attribute is a deletion (clause 4.22), so it needs the same
    /// temporal record as an explicit one — otherwise the history never shows
    /// the attribute going away and `attribute_current` keeps answering `q`
    /// filters with its last value. The payload rewrite and the tombstones
    /// share one transaction so neither can land without the other.
    async fn purge_expired_attributes(&self) -> Result<(), AndromedaError> {
        let mut uow = self.uow.begin().await?;
        let purged = uow.purge_expired_attributes().await?;
        if purged.is_empty() {
            return Ok(());
        }

        for entity in &purged {
            let removed = removed_attribute_instances(&entity.previous_payload, &entity.payload);
            uow.record_removed_attributes(&entity.id, &removed).await?;
        }
        uow.commit().await?;

        info!("Removed expired attribute instances from {} entity/entities", purged.len());
        if let Some(ref tx) = self.change_tx {
            for entity in purged {
                match entity.id.parse::<Uri>() {
                    Ok(entity_uri) => {
                        let _ = tx.send(ChangeEvent::new(entity_uri, Vec::new(), TriggerReason::EntityUpdated));
                    }
                    Err(_) => {
                        error!(entity_id = %entity.id, event = "entity_uri_parse_failed");
                    }
                }
            }
        }
        Ok(())
    }
}
