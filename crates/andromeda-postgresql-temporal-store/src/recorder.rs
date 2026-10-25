//! Writing typed temporal append intents in an existing anchor transaction:
//! as history, or as the records the intent queue holds them in.

use crate::{
    error::TemporalStoreError,
    intent_application::{apply_intents_in_tx, intent_source::IntentSource},
    repo::TemporalRepository,
    temporal_entry_record::{TemporalIntentRecord, mention_intent},
};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_attribute_instance::{name_keys::MentionedNames, name_resolution::resolve_mentioned};
use andromeda_temporal::append_intent::TemporalAppendIntent;
use sqlx::{Postgres, Transaction};
use std::sync::Arc;

/// Records typed temporal intents in the caller's PostgreSQL transaction.
#[derive(Debug)]
pub struct TemporalRecorder {
    /// Repository whose statements the recorder runs.
    repo: Arc<TemporalRepository>,
}

impl TemporalRecorder {
    /// Create a temporal recorder backed by the given repository.
    #[must_use]
    pub const fn new(repo: Arc<TemporalRepository>) -> TemporalRecorder {
        TemporalRecorder { repo }
    }

    /// Record typed append intents inside the caller's anchor transaction.
    ///
    /// `record` is the write identity every intent in `intents` shares. The caller states it explicitly rather than this method inferring it from `intents.first()`, and every intent is checked against it before anything is written: history rows are stamped with the record's identity, so an intent carrying another record would land its rows under an identity the commit never minted.
    ///
    /// The intents become the records the queue would hold, bound as one array, and the statements the drain applies queued intents with record them, so a commit and a drain write identical rows. Each row lands exactly once: a commit records its intents in its own transaction, a drain removes the intents it applies under their lease in the transaction that applies them, and a replayed side-effect job removes its job first in the transaction that replays it, so no path can apply one write's entries twice. `attribute_history` therefore carries no unique write identity to arbitrate on.
    ///
    /// History only. Current entity state is written by the same transaction, but by the crate that owns the commit rather than from here, and the temporal catalogue keeps Temporal Evolution independent of it in both directions.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::WriteRecordMismatch`] when an intent
    /// carries a write record other than `record`,
    /// [`TemporalStoreError::EntityIntent`] naming the entity whose intent
    /// cannot be stored, [`TemporalStoreError::NotFound`] when an intent
    /// records an observation against a Temporal Evolution that does not exist
    /// and declares no entity type to create it with,
    /// [`TemporalStoreError::DuplicateInstanceClaim`] when two instances of one
    /// attribute claim one `instanceId`, and [`TemporalStoreError::Database`]
    /// when a statement fails.
    pub async fn record_intents_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        record: &WriteRecord,
        intents: &[TemporalAppendIntent],
    ) -> Result<(), TemporalStoreError> {
        if intents.is_empty() {
            return Ok(());
        }
        let records = self.intent_records_in_tx(tx, record, intents).await?;
        apply_intents_in_tx(tx, &IntentSource::Bound { record, intents: &records }, target).await
    }

    /// The records `intents` wait in the intent queue as, every name they
    /// hold as a key minted once in the caller's transaction, so a key an
    /// entry holds exists exactly when the entry does.
    ///
    /// `record` is the write identity every intent shares; the queue stores it
    /// once per intent beside the record rather than in it.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::WriteRecordMismatch`] when an intent
    /// carries a write record other than `record`,
    /// [`TemporalStoreError::NameLookup`] when a dictionary cannot be
    /// resolved, and [`TemporalStoreError::EntityIntent`] naming the entity
    /// whose intent cannot be stored as a record.
    pub async fn intent_records_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        record: &WriteRecord,
        intents: &[TemporalAppendIntent],
    ) -> Result<Vec<TemporalIntentRecord>, TemporalStoreError> {
        refuse_foreign_records(record, intents)?;
        let mut mentioned = MentionedNames::default();
        for intent in intents {
            mention_intent(intent, &mut mentioned);
        }
        let keys = resolve_mentioned(tx, mentioned, self.repo.entity_type_names.as_ref(), self.repo.attribute_names.as_ref()).await?;
        intents
            .iter()
            .map(|intent| {
                TemporalIntentRecord::of(intent, &keys).map_err(|source| TemporalStoreError::EntityIntent {
                    entity_id: intent.entity_id.clone(), // clone: the failure owns the entity it names.
                    source: Box::new(source),
                })
            })
            .collect()
    }
}

/// Refuse a batch in which any intent carries a write record other than the
/// one its commit persists.
///
/// # Errors
///
/// Returns [`TemporalStoreError::WriteRecordMismatch`] naming the first intent
/// whose record differs from `record`.
fn refuse_foreign_records(record: &WriteRecord, intents: &[TemporalAppendIntent]) -> Result<(), TemporalStoreError> {
    match intents.iter().find(|intent| intent.record != *record) {
        Some(intent) => Err(TemporalStoreError::WriteRecordMismatch {
            write_id: intent.record.write_id(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{error::TemporalStoreError, recorder::refuse_foreign_records};
    use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
    use andromeda_temporal::append_intent::TemporalAppendIntent;
    use chrono::{TimeZone, Utc};

    /// An intent for one entity, stamped with `record`.
    fn intent(record: WriteRecord) -> TemporalAppendIntent {
        TemporalAppendIntent {
            record,
            base_ordinal: WriteOrdinal::at(0),
            entity_id: "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI"),
            entity_types: Vec::new(),
            entries: Vec::new(),
        }
    }

    #[test]
    fn an_intent_stamped_with_a_different_write_record_is_refused() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let persisted = WriteRecord::mint(instant);
        let foreign = WriteRecord::mint(instant);
        assert!(refuse_foreign_records(&persisted, &[intent(persisted)]).is_ok());
        let refused = refuse_foreign_records(&persisted, &[intent(persisted), intent(foreign)]);
        assert!(
            matches!(refused, Err(TemporalStoreError::WriteRecordMismatch { write_id }) if write_id == foreign.write_id()),
            "{refused:?}"
        );
    }
}
