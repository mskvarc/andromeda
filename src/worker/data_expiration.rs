//! Scheduling entity and attribute-instance expiry policy (clause 4.22).

use andromeda_entity::{EntityService, batch_activity::BatchActivityTracker, port::expiry::EntityExpirySource};
use std::{sync::Arc, time::Duration};
use tracing::{debug, error, info};

/// Combined worker that expires both entities and attribute instances.
pub struct DataExpirationWorker {
    entity_policy: Arc<EntityService>,
    expiry_source: Arc<dyn EntityExpirySource>,
    interval: Duration,
    batch_activity: BatchActivityTracker,
}

impl DataExpirationWorker {
    /// Create the worker for one tenant.
    pub const fn new(
        entity_policy: Arc<EntityService>,
        expiry_source: Arc<dyn EntityExpirySource>,
        interval_secs: u64,
        batch_activity: BatchActivityTracker,
    ) -> DataExpirationWorker {
        DataExpirationWorker {
            entity_policy,
            expiry_source,
            interval: Duration::from_secs(interval_secs),
            batch_activity,
        }
    }

    /// Sweep on a fixed cadence until the task is aborted.
    pub async fn run(self) {
        info!("DataExpirationWorker starting (interval: {:?})", self.interval);
        let mut ticker = tokio::time::interval(self.interval);

        loop {
            ticker.tick().await;

            if self.batch_activity.is_within_cooldown() {
                debug!("DataExpirationWorker skipping scan: batch activity within cooldown");
                continue;
            }

            match self.entity_policy.expire_due_entities(self.expiry_source.as_ref()).await {
                Ok(expired_count) => {
                    if expired_count > 0 {
                        info!("Expired {expired_count} entity/entities");
                    }
                }
                Err(err) => {
                    error!("DataExpirationWorker failed to expire entities: {err}");
                }
            }

            match self.entity_policy.purge_expired_attributes(self.expiry_source.as_ref()).await {
                Ok(purged_count) if purged_count > 0 => info!("Removed expired attributes from {purged_count} entity/entities"),
                Ok(_) => {}
                Err(error) => error!("DataExpirationWorker failed to expire attributes: {error}"),
            }
        }
    }
}
