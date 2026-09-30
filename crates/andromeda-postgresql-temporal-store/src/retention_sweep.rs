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
use andromeda_postgresql_schema::temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams, TemporalEntities};
use andromeda_postgresql_seaquery::timescale::drop_chunks;
use chrono::{DateTime, TimeDelta, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, Iden, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::AssertSqlSafe;
use thiserror::Error;

/// The `attribute_history` alias inside a garbage-collection subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
struct Ah;

/// The `temporal_attribute_streams` alias inside a garbage-collection subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The `scope_history` alias inside a garbage-collection subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "sh"]
struct Sh;

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
        let cutoff = horizon.cutoff(now).map_err(TemporalStoreError::RetentionHorizon)?;
        let attribute_history_chunks = self.drop_chunks("attribute_history", cutoff).await?;
        let scope_history_chunks = self.drop_chunks("scope_history", cutoff).await?;
        let (streams_sql, streams_values) = collect_empty_streams().build_sqlx(PostgresQueryBuilder);
        let streams = sqlx::query_with(AssertSqlSafe(streams_sql), streams_values)
            .execute(&self.pool)
            .await?
            .rows_affected();
        let (entities_sql, entities_values) = collect_empty_temporal_entities().build_sqlx(PostgresQueryBuilder);
        let temporal_entities = sqlx::query_with(AssertSqlSafe(entities_sql), entities_values)
            .execute(&self.pool)
            .await?
            .rows_affected();
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
    async fn drop_chunks(&self, hypertable: &str, cutoff: DateTime<Utc>) -> Result<usize, TemporalStoreError> {
        let statement = Query::select().expr(drop_chunks(hypertable, Expr::val(cutoff))).to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?.len())
    }
}

// `drop_chunks` removes whole chunks whose range ends before the cutoff, so the
// oldest surviving row can be older than the cutoff by up to one chunk
// interval. That is the intended behaviour: retention ages storage out at chunk
// granularity and never rewrites a chunk to trim rows out of it.

/// Garbage-collect stream catalogue rows no history row names.
///
/// A stream is created in the same transaction as the first history row that
/// names it, so a stream with no history is a stream whose history has left
/// storage rather than one a concurrent write has not filled in yet.
fn collect_empty_streams() -> DeleteStatement {
    let no_history = Query::select()
        .expr(Expr::val(1))
        .from_as(AttributeHistory::Table, Ah)
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)))
        .to_owned();
    Query::delete()
        .from_table(TemporalAttributeStreams::Table)
        .and_where(Expr::exists(no_history).not())
        .to_owned()
}

/// Garbage-collect temporal-entity rows nothing hangs off: no stream and no
/// scope-history row references them.
fn collect_empty_temporal_entities() -> DeleteStatement {
    let no_stream = Query::select()
        .expr(Expr::val(1))
        .from_as(TemporalAttributeStreams::Table, S)
        .and_where(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((TemporalEntities::Table, TemporalEntities::TemporalEntityKey)))
        .to_owned();
    let no_scope = Query::select()
        .expr(Expr::val(1))
        .from_as(ScopeHistory::Table, Sh)
        .and_where(Expr::col((Sh, ScopeHistory::TemporalEntityKey)).equals((TemporalEntities::Table, TemporalEntities::TemporalEntityKey)))
        .to_owned();
    Query::delete()
        .from_table(TemporalEntities::Table)
        .and_where(Expr::exists(no_stream).not())
        .and_where(Expr::exists(no_scope).not())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::retention_sweep::{
        TemporalRetention,
        TemporalRetentionHorizon,
        TemporalRetentionHorizonInvalid,
        collect_empty_streams,
        collect_empty_temporal_entities,
    };
    use andromeda_postgresql_seaquery::timescale::drop_chunks;
    use chrono::{DateTime, TimeDelta, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

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
        for hypertable in ["attribute_history", "scope_history"] {
            let sql = Query::select()
                .expr(drop_chunks(hypertable, Expr::val("2026-01-01T00:00:00Z")))
                .to_owned()
                .to_string(PostgresQueryBuilder);
            assert!(sql.contains(&format!("drop_chunks('{hypertable}', older_than => ")), "{sql}");
            assert!(sql.contains("::timestamptz"), "{sql}");
            assert!(!sql.contains("observed_at"), "{sql}");
        }
    }

    #[test]
    fn garbage_collection_removes_only_catalogue_rows_nothing_names() {
        let streams = collect_empty_streams().to_string(PostgresQueryBuilder);
        assert!(
            streams.contains(r#"NOT EXISTS(SELECT 1 FROM "attribute_history" AS "ah" WHERE "ah"."stream_key" = "temporal_attribute_streams"."stream_key")"#),
            "{streams}"
        );
        let entities = collect_empty_temporal_entities().to_string(PostgresQueryBuilder);
        assert!(
            entities.contains(r#""s"."temporal_entity_key" = "temporal_entities"."temporal_entity_key""#),
            "{entities}"
        );
        assert!(
            entities.contains(r#""sh"."temporal_entity_key" = "temporal_entities"."temporal_entity_key""#),
            "{entities}"
        );
        assert_eq!(entities.matches("NOT EXISTS").count(), 2, "{entities}");
    }

    #[test]
    fn no_garbage_collection_statement_records_an_ngsi_ld_deletion() {
        for sql in [
            collect_empty_streams().to_string(PostgresQueryBuilder),
            collect_empty_temporal_entities().to_string(PostgresQueryBuilder),
        ] {
            assert!(!sql.contains("INSERT"), "{sql}");
            assert!(!sql.contains("deleted_at"), "{sql}");
        }
    }
}
