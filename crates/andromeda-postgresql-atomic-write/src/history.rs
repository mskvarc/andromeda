//! Native PostgreSQL history writes inside the context transaction.

use andromeda_context_commit::{atomic_write::AtomicWrite, error::CommitError, history_record::HistoryRecord};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use std::sync::Arc;

/// Persist every landed write's native history and return projection work that
/// may be published only after the transaction commits.
pub(crate) async fn apply(
    recorder: &TemporalRecorder,
    tx: &mut Transaction<'_, Postgres>,
    landed: &[&AtomicWrite],
) -> Result<Vec<HistoryBatchEntry>, CommitError> {
    let observations = observations(landed);
    let mut written = if observations.is_empty() {
        Vec::new()
    } else {
        recorder
            .record_entities_batch_in_tx(tx, &observations)
            .await
            .map_err(|error| CommitError::Persistence { detail: error.to_string() })?
    };
    for write in landed {
        for record in &write.history {
            let HistoryRecord::Tombstone(key) = record else {
                continue;
            };
            let tombstone = recorder
                .record_attribute_deletion_in_tx(tx, &write.entity_id, &key.attr, key.dataset_id.as_ref())
                .await
                .map_err(|error| CommitError::Persistence { detail: error.to_string() })?;
            written.extend(tombstone);
        }
    }
    Ok(written)
}

/// Borrow observation records out of landed writes as shared owned handles.
fn observations(landed: &[&AtomicWrite]) -> Vec<(Arc<Uri>, Arc<Value>)> {
    landed
        .iter()
        .flat_map(|write| {
            write.history.iter().filter_map(|record| match record {
                HistoryRecord::Observations(payload) => Some((Arc::clone(&write.entity_id), Arc::clone(payload))),
                HistoryRecord::Tombstone(_) => None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::history::observations;
    use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, history_record::HistoryRecord};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn observations_preserve_entity_pairing_and_shared_payloads() {
        let payload = Arc::new(json!([{"@id": "urn:ngsi-ld:A"}]));
        let write = AtomicWrite::builder()
            .entity_id(Arc::new("urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI")))
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::Observations(Arc::clone(&payload))])
            .build();
        let collected = observations(&[&write]);
        pretty_assertions::assert_eq!(collected[0].0.as_str(), "urn:ngsi-ld:A");
        assert!(Arc::ptr_eq(&payload, &collected[0].1));
    }
}
