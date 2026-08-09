//! Expiring entities and attribute instances whose `expiresAt` has passed
//! (clause 4.22).

use crate::{batch_activity::BatchActivityTracker, error::AndromedaError, service::entity::store::EntityStore};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    committer::ContextCommitter,
    entity_mutation::{EntityMutation, EntityRecord},
    history_record::HistoryRecord,
    precondition::Precondition,
    write_outcome::WriteOutcome,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    expanded::{instance_key::removed_attribute_instances, mutation::expiry::remove_expired_attribute_instances},
    notification::NotificationTrigger,
};
use andromeda_postgresql_context_store::entity::snapshot::EntitySnapshot;
use chrono::Utc;
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Combined worker that expires both entities and attribute instances.
pub struct DataExpirationWorker {
    entity_repo: Arc<dyn EntityStore>,
    writes: Arc<dyn ContextCommitter>,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    interval: Duration,
    batch_activity: BatchActivityTracker,
}

impl DataExpirationWorker {
    /// Create the worker for one tenant.
    pub const fn new(
        entity_repo: Arc<dyn EntityStore>,
        writes: Arc<dyn ContextCommitter>,
        change_tx: Option<broadcast::Sender<ChangeEvent>>,
        interval_secs: u64,
        batch_activity: BatchActivityTracker,
    ) -> DataExpirationWorker {
        DataExpirationWorker {
            entity_repo,
            writes,
            change_tx,
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

            match self.entity_repo.expire_due_entities().await {
                Ok(expired_ids) => {
                    if !expired_ids.is_empty() {
                        info!("Expired {} entity/entities", expired_ids.len());
                        if let Some(ref tx) = self.change_tx {
                            for id in expired_ids {
                                let _ = tx.send(ChangeEvent::new(id, Vec::new(), NotificationTrigger::EntityDeleted));
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
    /// filters with its last value. Each entity is one atomic write, so the
    /// payload rewrite and its tombstones land together and one entity failing
    /// does not undo the entities already purged in this sweep.
    ///
    /// Each write states the `modifiedAt` its removal was computed from. A
    /// concurrent writer simply loses the race and the entity is picked up on
    /// the next sweep, because an instance that was not removed is still
    /// past due.
    async fn purge_expired_attributes(&self) -> Result<(), AndromedaError> {
        let candidates = self.entity_repo.entities_with_expired_attributes().await?;
        if candidates.is_empty() {
            return Ok(());
        }

        let cutoff = Utc::now();
        let mut writes = Vec::with_capacity(candidates.len());
        let mut purged_ids = Vec::with_capacity(candidates.len());
        for (id, snapshot) in candidates {
            let Some(write) = purge_write(&id, snapshot, cutoff) else {
                continue;
            };
            writes.push(write);
            purged_ids.push(id);
        }

        if writes.is_empty() {
            return Ok(());
        }

        let outcomes = self.writes.commit(writes).await?;
        let purged = landed_ids(purged_ids, &outcomes);
        if purged.is_empty() {
            return Ok(());
        }

        info!("Removed expired attribute instances from {} entity/entities", purged.len());
        if let Some(ref tx) = self.change_tx {
            for id in purged {
                let _ = tx.send(ChangeEvent::new(id, Vec::new(), NotificationTrigger::EntityUpdated));
            }
        }
        Ok(())
    }
}

/// The identifiers whose purge actually landed.
///
/// Positional: the coordinator answers one outcome per submitted write in
/// submission order. A write a concurrent update superseded removed nothing, so
/// it must neither be counted nor notified — the entity is still past due and
/// the next sweep picks it up. Notifying for it would tell every matching
/// subscriber about a change that did not happen.
fn landed_ids(purged_ids: Vec<Uri>, outcomes: &[WriteOutcome]) -> Vec<Uri> {
    purged_ids
        .into_iter()
        .zip(outcomes)
        .filter(|(_, outcome)| outcome.is_updated())
        .map(|(id, _)| id)
        .collect()
}

/// The write that removes `snapshot`'s past-due instances, or `None` when the
/// candidate turns out to hold none by the time it is read.
fn purge_write(id: &Uri, snapshot: EntitySnapshot, cutoff: chrono::DateTime<Utc>) -> Option<AtomicWrite> {
    let EntitySnapshot {
        payload: previous,
        modified_at,
    } = snapshot;
    let mut purged = previous.clone(); // clone: the "before" side stays intact for the removal diff
    if !remove_expired_attribute_instances(&mut purged, cutoff) {
        return None;
    }

    let history: Vec<HistoryRecord> = removed_attribute_instances(&previous, &purged)
        .into_iter()
        .map(HistoryRecord::Tombstone)
        .collect();

    Some(
        AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
            .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::new(purged))))
            .history(history)
            .precondition(Precondition::UnchangedSince(modified_at))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::{landed_ids, purge_write};
    use andromeda_context_commit::{history_record::HistoryRecord, write_outcome::WriteOutcome};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_postgresql_context_store::entity::snapshot::EntitySnapshot;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn ids(raw: &[&str]) -> Vec<Uri> {
        raw.iter().map(|id| id.parse().expect("valid test uri")).collect()
    }

    #[test]
    fn only_the_entities_whose_purge_landed_are_counted_and_notified() {
        // A write a concurrent update superseded removed nothing. Notifying for
        // it would tell every matching subscriber that an entity changed when it
        // did not, and the entity is still past due for the next sweep.
        let outcomes = vec![WriteOutcome::Updated, WriteOutcome::Superseded, WriteOutcome::Updated];
        pretty_assertions::assert_eq!(
            landed_ids(ids(&["urn:ngsi-ld:A", "urn:ngsi-ld:B", "urn:ngsi-ld:C"]), &outcomes),
            ids(&["urn:ngsi-ld:A", "urn:ngsi-ld:C"])
        );
    }

    #[test]
    fn a_sweep_whose_every_write_was_superseded_notifies_nobody() {
        let outcomes = vec![WriteOutcome::Superseded, WriteOutcome::Superseded];
        assert!(landed_ids(ids(&["urn:ngsi-ld:A", "urn:ngsi-ld:B"]), &outcomes).is_empty());
    }

    #[test]
    fn an_entity_that_vanished_mid_sweep_is_neither_counted_nor_notified() {
        let outcomes = vec![WriteOutcome::NotFound, WriteOutcome::Updated];
        pretty_assertions::assert_eq!(landed_ids(ids(&["urn:ngsi-ld:A", "urn:ngsi-ld:B"]), &outcomes), ids(&["urn:ngsi-ld:B"]));
    }

    fn cutoff() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").expect("cutoff parses").with_timezone(&Utc)
    }

    fn snapshot(payload: Value) -> EntitySnapshot {
        EntitySnapshot {
            payload,
            modified_at: cutoff(),
        }
    }

    fn expiring_instance(expires_at: &str) -> Value {
        json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": expires_at}]
        })
    }

    #[test]
    fn a_past_due_instance_is_written_back_with_a_tombstone() {
        let payload = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [expiring_instance("2025-01-01T00:00:00Z")]}]);
        let id = "urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI");
        let write = purge_write(&id, snapshot(payload), cutoff()).expect("a purge write");

        pretty_assertions::assert_eq!(write.history.len(), 1);
        assert!(matches!(write.history[0], HistoryRecord::Tombstone(_)));
        assert!(write.precondition.is_unchanged_since());
    }

    #[test]
    fn an_entity_with_nothing_past_due_produces_no_write() {
        let payload = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [expiring_instance("2027-01-01T00:00:00Z")]}]);
        let id = "urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI");
        assert!(purge_write(&id, snapshot(payload), cutoff()).is_none());
    }
}
