//! Appending the history part of a set of atomic writes.
//!
//! Every observation across the whole set goes in one call, because the
//! catalogue upsert and the history insert are each one statement no matter how
//! many entities they carry. Tombstones stay one call apiece: each one first
//! reads the catalogue to learn what the attribute is, which is per-attribute
//! work with no bulk form.

use crate::{
    error::AndromedaError,
    service::entity::{atomic_write::AtomicWrite, history_record::HistoryRecord},
};
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use iri_rs::IriBuf;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use std::sync::Arc;

/// Write the history of every landed write, returning the rows written so the
/// caller can project them once the transaction commits.
pub(super) async fn apply(
    recorder: &TemporalRecorder,
    tx: &mut Transaction<'_, Postgres>,
    landed: &[&AtomicWrite],
) -> Result<Vec<HistoryBatchEntry>, AndromedaError> {
    let observations: Vec<(Arc<str>, Arc<Value>)> = landed
        .iter()
        .flat_map(|write| {
            write.history.iter().filter_map(|record| match record {
                // clone: both halves are `Arc` handles shared with the change-event
                // path; the recorder takes them by value.
                HistoryRecord::Observations(payload) => Some((Arc::clone(&write.entity_id), Arc::clone(payload))),
                HistoryRecord::Tombstone(_) => None,
            })
        })
        .collect();

    let mut written = if observations.is_empty() {
        Vec::new()
    } else {
        recorder.record_entities_batch_in_tx(tx, &observations).await?
    };

    for write in landed {
        for record in &write.history {
            let HistoryRecord::Tombstone(key) = record else {
                continue;
            };
            let tombstone = recorder
                .record_attribute_deletion_in_tx(tx, &write.entity_id, key.attr.as_str(), key.dataset_id.as_ref().map(IriBuf::as_str))
                .await?;
            written.extend(tombstone);
        }
    }

    Ok(written)
}
