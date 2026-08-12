//! Operator retention: ageing temporal storage out, never NGSI-LD deletion.
//!
//! Retention is a storage decision an operator takes about their own tenant,
//! and the specification knows nothing about it. A chunk dropped here is data
//! leaving storage, not a Temporal Evolution deletion (clause 5.6.16): no
//! tombstone is recorded, no notification is raised, and a temporal query that
//! afterwards finds nothing is answering about storage the operator chose not
//! to keep. Retention is therefore off unless an operator turns it on.
//!
//! The sweep has three parts, in order: chunks older than the horizon are
//! dropped from both hypertables, the catalogue rows whose history left with
//! them are garbage-collected, and the idempotency records that are fully
//! behind the horizon and protect nothing are released.

use crate::{error::TemporalStoreError, repo::TemporalRepository, write_record_release::release_behind_horizon};
use chrono::{DateTime, TimeDelta, Utc};
use thiserror::Error;

/// Whether an operator has asked for temporal storage to be aged out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TemporalRetention {
    /// Nothing is ever dropped. This is what a broker does unless configured.
    #[default]
    Disabled,
    /// Storage older than the horizon is dropped on every sweep.
    Enabled {
        /// How far back storage is kept.
        horizon: TemporalRetentionHorizon,
    },
}

/// How far back an operator keeps temporal storage.
///
/// The horizon is a positive span: a zero or negative one would place the
/// cutoff at or after the present and drop history the broker is still writing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalRetentionHorizon(TimeDelta);

impl TemporalRetentionHorizon {
    /// Accept a configured retention span.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalRetentionHorizonInvalid`] when the span is not positive.
    pub fn new(span: TimeDelta) -> Result<TemporalRetentionHorizon, TemporalRetentionHorizonInvalid> {
        if span > TimeDelta::zero() {
            Ok(TemporalRetentionHorizon(span))
        } else {
            Err(TemporalRetentionHorizonInvalid)
        }
    }

    /// The instant before which storage is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalRetentionHorizonInvalid`] when subtracting the span
    /// from the given instant leaves the representable range.
    pub fn cutoff(self, now: DateTime<Utc>) -> Result<DateTime<Utc>, TemporalRetentionHorizonInvalid> {
        now.checked_sub_signed(self.0).ok_or(TemporalRetentionHorizonInvalid)
    }
}

/// A configured retention span does not name a usable cutoff.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("a temporal retention horizon is a positive span that names an instant in the past")]
pub struct TemporalRetentionHorizonInvalid;

/// What one retention pass did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemporalRetentionOutcome {
    /// Retention is off, so the pass touched nothing.
    Disabled,
    /// Retention ran against the reported cutoff.
    Swept(TemporalRetentionSweep),
}

/// What one retention sweep removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalRetentionSweep {
    /// The instant before which storage was dropped.
    pub cutoff: DateTime<Utc>,
    /// Attribute history chunks dropped.
    pub attribute_history_chunks: usize,
    /// Scope history chunks dropped.
    pub scope_history_chunks: usize,
    /// Streams garbage-collected because no history row names them.
    pub streams: u64,
    /// Temporal entities garbage-collected because nothing hangs off them.
    pub temporal_entities: u64,
    /// Idempotency records released behind the horizon.
    pub released_write_records: u64,
}

impl TemporalRepository {
    /// Run one operator retention pass.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Validation`] when the configured horizon
    /// names no usable cutoff and [`TemporalStoreError::Database`] when a
    /// statement fails.
    pub async fn sweep_retention(&self, retention: TemporalRetention, now: DateTime<Utc>) -> Result<TemporalRetentionOutcome, TemporalStoreError> {
        let horizon = match retention {
            TemporalRetention::Disabled => return Ok(TemporalRetentionOutcome::Disabled),
            TemporalRetention::Enabled { horizon } => horizon,
        };
        let cutoff = horizon.cutoff(now).map_err(|error| TemporalStoreError::Validation(error.to_string()))?;
        let attribute_history_chunks = self.drop_chunks(DROP_ATTRIBUTE_HISTORY_CHUNKS_SQL, cutoff).await?;
        let scope_history_chunks = self.drop_chunks(DROP_SCOPE_HISTORY_CHUNKS_SQL, cutoff).await?;
        let streams = sqlx::query(COLLECT_EMPTY_STREAMS_SQL).execute(&self.pool).await?.rows_affected();
        let temporal_entities = sqlx::query(COLLECT_EMPTY_TEMPORAL_ENTITIES_SQL).execute(&self.pool).await?.rows_affected();
        let released_write_records = release_behind_horizon(&self.pool, cutoff).await?;
        Ok(TemporalRetentionOutcome::Swept(TemporalRetentionSweep {
            cutoff,
            attribute_history_chunks,
            scope_history_chunks,
            streams,
            temporal_entities,
            released_write_records,
        }))
    }

    /// Drop one hypertable's chunks that lie entirely before the cutoff.
    async fn drop_chunks(&self, statement: &str, cutoff: DateTime<Utc>) -> Result<usize, TemporalStoreError> {
        Ok(sqlx::query(statement).bind(cutoff).fetch_all(&self.pool).await?.len())
    }
}

// `drop_chunks` removes whole chunks whose range ends before the cutoff, so the
// oldest surviving row can be older than the cutoff by up to one chunk
// interval. That is the intended behaviour: retention ages storage out at chunk
// granularity and never rewrites a chunk to trim rows out of it.
const DROP_ATTRIBUTE_HISTORY_CHUNKS_SQL: &str = r#"
SELECT drop_chunks('attribute_history', older_than => $1::timestamptz)
"#;

const DROP_SCOPE_HISTORY_CHUNKS_SQL: &str = r#"
SELECT drop_chunks('scope_history', older_than => $1::timestamptz)
"#;

// A stream is created in the same transaction as the first history row that
// names it, so a stream with no history is a stream whose history has left
// storage rather than one a concurrent write has not filled in yet.
const COLLECT_EMPTY_STREAMS_SQL: &str = r#"
DELETE FROM temporal_attribute_streams s
WHERE NOT EXISTS (SELECT 1 FROM attribute_history ah WHERE ah.stream_id = s.id)
"#;

const COLLECT_EMPTY_TEMPORAL_ENTITIES_SQL: &str = r#"
DELETE FROM temporal_entities te
WHERE NOT EXISTS (SELECT 1 FROM temporal_attribute_streams s WHERE s.temporal_entity_key = te.entity_key)
  AND NOT EXISTS (SELECT 1 FROM scope_history sh WHERE sh.temporal_entity_key = te.entity_key)
"#;

#[cfg(test)]
mod tests {
    use crate::retention_sweep::{
        COLLECT_EMPTY_STREAMS_SQL,
        COLLECT_EMPTY_TEMPORAL_ENTITIES_SQL,
        DROP_ATTRIBUTE_HISTORY_CHUNKS_SQL,
        DROP_SCOPE_HISTORY_CHUNKS_SQL,
        TemporalRetention,
        TemporalRetentionHorizon,
        TemporalRetentionHorizonInvalid,
    };
    use chrono::{DateTime, TimeDelta, Utc};

    /// The fixed instant the cutoff cases are measured from.
    fn now() -> Result<DateTime<Utc>, String> {
        "2026-08-11T12:00:00Z".parse::<DateTime<Utc>>().map_err(|error| error.to_string())
    }

    #[test]
    fn retention_is_off_unless_an_operator_turns_it_on() {
        assert_eq!(TemporalRetention::default(), TemporalRetention::Disabled);
    }

    #[test]
    fn a_non_positive_horizon_is_refused() {
        assert_eq!(TemporalRetentionHorizon::new(TimeDelta::zero()), Err(TemporalRetentionHorizonInvalid));
        assert_eq!(TemporalRetentionHorizon::new(TimeDelta::days(-1)), Err(TemporalRetentionHorizonInvalid));
    }

    #[test]
    fn a_positive_horizon_names_a_cutoff_that_far_back() {
        let cutoff = TemporalRetentionHorizon::new(TimeDelta::days(30))
            .map_err(|error| error.to_string())
            .and_then(|horizon| now().and_then(|now| horizon.cutoff(now).map_err(|error| error.to_string())));
        assert_eq!(
            cutoff.map(|cutoff| cutoff.to_rfc3339()),
            now().map(|now| (now - TimeDelta::days(30)).to_rfc3339())
        );
    }

    #[test]
    fn both_hypertables_are_aged_out_on_the_operational_write_clock() {
        for statement in [DROP_ATTRIBUTE_HISTORY_CHUNKS_SQL, DROP_SCOPE_HISTORY_CHUNKS_SQL] {
            assert!(statement.contains("older_than => $1::timestamptz"));
            assert!(!statement.contains("observed_at"));
        }
        assert!(DROP_ATTRIBUTE_HISTORY_CHUNKS_SQL.contains("'attribute_history'"));
        assert!(DROP_SCOPE_HISTORY_CHUNKS_SQL.contains("'scope_history'"));
    }

    #[test]
    fn garbage_collection_removes_only_catalogue_rows_nothing_names() {
        assert!(COLLECT_EMPTY_STREAMS_SQL.contains("NOT EXISTS (SELECT 1 FROM attribute_history ah WHERE ah.stream_id = s.id)"));
        assert!(COLLECT_EMPTY_TEMPORAL_ENTITIES_SQL.contains("FROM temporal_attribute_streams s WHERE s.temporal_entity_key = te.entity_key"));
        assert!(COLLECT_EMPTY_TEMPORAL_ENTITIES_SQL.contains("FROM scope_history sh WHERE sh.temporal_entity_key = te.entity_key"));
    }

    #[test]
    fn no_retention_statement_records_an_ngsi_ld_deletion() {
        for statement in [
            DROP_ATTRIBUTE_HISTORY_CHUNKS_SQL,
            DROP_SCOPE_HISTORY_CHUNKS_SQL,
            COLLECT_EMPTY_STREAMS_SQL,
            COLLECT_EMPTY_TEMPORAL_ENTITIES_SQL,
        ] {
            assert!(!statement.contains("INSERT"));
            assert!(!statement.contains("deleted_at"));
        }
    }
}
