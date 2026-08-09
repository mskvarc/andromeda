//! Applying clause 4.22 expiry through the ordinary atomic mutation boundary.

use crate::{EntityService, entity_snapshot::EntitySnapshot, error::EntityError, port::expiry::EntityExpirySource};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
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
use chrono::{DateTime, Utc};
use std::sync::Arc;

impl EntityService {
    /// Delete every entity-level expiry candidate through normal entity deletion policy.
    pub async fn expire_due_entities(&self, source: &dyn EntityExpirySource) -> Result<usize, EntityError> {
        let candidates = source.entities_due_for_expiry().await?;
        let mut expired = 0usize;
        for entity_id in candidates {
            match self.delete(&entity_id).await {
                Ok(()) => expired += 1,
                Err(EntityError::NotFound { identifier: _ }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(expired)
    }

    /// Remove every past-due attribute instance and record its history tombstone atomically.
    pub async fn purge_expired_attributes(&self, source: &dyn EntityExpirySource) -> Result<usize, EntityError> {
        let candidates = source.entities_with_expired_attributes().await?;
        let cutoff = Utc::now();
        let (writes, identifiers): (Vec<_>, Vec<_>) = candidates
            .into_iter()
            .filter_map(|(identifier, snapshot)| purge_write(&identifier, snapshot, cutoff).map(|write| (write, identifier)))
            .unzip();
        if writes.is_empty() {
            return Ok(0);
        }

        let outcomes = self.writes.commit(writes).await?;
        let landed = landed_ids(identifiers, &outcomes);
        landed.iter().for_each(|identifier| {
            self.emit_change(ChangeEvent::new(
                identifier.clone(), // clone: the committed event owns its entity identifier.
                Vec::new(),
                NotificationTrigger::EntityUpdated,
            ));
        });
        Ok(landed.len())
    }
}

fn landed_ids(identifiers: Vec<Uri>, outcomes: &[WriteOutcome]) -> Vec<Uri> {
    identifiers
        .into_iter()
        .zip(outcomes)
        .filter(|(_, outcome)| outcome.is_updated())
        .map(|(identifier, _)| identifier)
        .collect()
}

fn purge_write(identifier: &Uri, snapshot: EntitySnapshot, cutoff: DateTime<Utc>) -> Option<AtomicWrite> {
    let EntitySnapshot {
        payload: previous,
        modified_at,
    } = snapshot;
    let mut purged = previous.clone(); // clone: history diffing requires both the before and after payloads.
    if !remove_expired_attribute_instances(&mut purged, cutoff) {
        return None;
    }
    let history = removed_attribute_instances(&previous, &purged)
        .into_iter()
        .map(HistoryRecord::Tombstone)
        .collect();
    Some(
        AtomicWrite::builder()
            .entity_id(Arc::new(identifier.clone())) // clone: the atomic write owns a shared validated identifier.
            .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::new(purged))))
            .history(history)
            .precondition(Precondition::UnchangedSince(modified_at))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_snapshot::EntitySnapshot,
        expiry::{landed_ids, purge_write},
    };
    use andromeda_context_commit::{history_record::HistoryRecord, write_outcome::WriteOutcome};
    use andromeda_ngsi_ld_lexicon::uri::{Uri, UriParseError};
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn identifiers(raw: &[&str]) -> Result<Vec<Uri>, UriParseError> {
        raw.iter().map(|identifier| identifier.parse()).collect()
    }

    const fn cutoff() -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH
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
    fn only_successful_purges_are_reported() -> Result<(), UriParseError> {
        let outcomes = vec![WriteOutcome::Updated, WriteOutcome::Superseded, WriteOutcome::Updated];
        pretty_assertions::assert_eq!(
            landed_ids(identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:B", "urn:ngsi-ld:C"])?, &outcomes),
            identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:C"])?
        );
        Ok(())
    }

    #[test]
    fn vanished_and_superseded_entities_are_not_reported() -> Result<(), UriParseError> {
        let outcomes = vec![WriteOutcome::NotFound, WriteOutcome::Superseded];
        assert!(landed_ids(identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:B"])?, &outcomes).is_empty());
        Ok(())
    }

    #[test]
    fn a_past_due_instance_produces_an_optimistic_write_and_tombstone() -> Result<(), UriParseError> {
        let payload = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [expiring_instance("1969-01-01T00:00:00Z")]}]);
        let identifier = "urn:ngsi-ld:A".parse::<Uri>()?;
        let write = purge_write(&identifier, snapshot(payload), cutoff());
        assert!(write.is_some());
        let Some(write) = write else {
            return Ok(());
        };
        pretty_assertions::assert_eq!(write.history.len(), 1);
        assert!(matches!(write.history[0], HistoryRecord::Tombstone(_)));
        assert!(write.precondition.is_unchanged_since());
        Ok(())
    }

    #[test]
    fn a_future_instance_produces_no_write() -> Result<(), UriParseError> {
        let payload = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [expiring_instance("1971-01-01T00:00:00Z")]}]);
        let identifier = "urn:ngsi-ld:A".parse::<Uri>()?;
        assert!(purge_write(&identifier, snapshot(payload), cutoff()).is_none());
        Ok(())
    }
}
