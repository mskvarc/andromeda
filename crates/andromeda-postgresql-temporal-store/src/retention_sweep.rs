//! Operator retention: ageing temporal storage out, never NGSI-LD deletion.
//!
//! Retention is a storage decision an operator takes about their own tenant,
//! and the specification knows nothing about it. A chunk dropped here is data
//! leaving storage, not a Temporal Evolution deletion (clause 5.6.16): no
//! tombstone is recorded, no notification is raised, and a temporal query that
//! afterwards finds nothing is answering about storage the operator chose not
//! to keep. Retention is therefore off unless an operator turns it on.
//!
//! The sweep has four parts, in order: attribute history chunks older than the
//! horizon are dropped, the Scope events no instance retained after the
//! horizon is read under are deleted, the catalogue rows whose history left
//! with them are garbage-collected, and the idempotency records that are fully
//! behind the horizon and protect nothing are released. Each statement runs in
//! a transaction of its own with no statement timeout: it removes however much
//! has aged out, and no request waits on it.
//!
//! Scope history is not dropped by age. Clause 4.18 keeps a Scope valid "from
//! the time it has been set until the time it has been explicitly removed", so
//! the event that set the Scope in force at the cutoff is part of the meaning
//! of every attribute instance kept after it, however old the event is. The
//! sweep deletes only the events superseded at or before the cutoff on both
//! the `recorded_at` and the `observedAt` axis, which keeps per Entity the
//! state in force at the cutoff on each; `createdAt` and `modifiedAt` order
//! every Scope event as `recorded_at` does, and `deletedAt` carries no Scope.
//! An Entity whose attribute history has left storage altogether keeps no
//! instance to read under a Scope, so its Scope events go once all of them lie
//! behind the cutoff.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    statement_timeout_allowance::allow_unbounded_statements,
    write_record_release::release_behind_horizon,
};
use andromeda_postgresql_schema::temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams, TemporalEntities};
use andromeda_postgresql_seaquery::timescale::drop_chunks;
use chrono::{DateTime, TimeDelta, Utc};
use sea_query::{Condition, DeleteStatement, Expr, ExprTrait, Iden, PostgresQueryBuilder, Query};
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

/// The `scope_history` alias of a later Scope event that supersedes the one
/// the sweep considers.
#[derive(Iden, Clone, Copy)]
#[iden = "sn"]
struct Sn;

/// The axes a Scope event must be superseded on before the sweep deletes it.
const SUPERSEDED_AXES: [ScopeHistory; 2] = [ScopeHistory::RecordedAt, ScopeHistory::ObservedAt];

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
    /// Scope events deleted: superseded at or before the cutoff, or of an
    /// Entity whose attribute history has left storage.
    pub scope_events: u64,
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
        let streams = self.delete(&collect_empty_streams()).await?;
        let scope_events = self.delete(&superseded_scope_events(cutoff)).await? + self.delete(&streamless_scope_events(cutoff)).await?;
        let temporal_entities = self.delete(&collect_empty_temporal_entities()).await?;
        let released_write_records = release_behind_horizon(&self.pool, cutoff).await?;
        Ok(TemporalRetentionOutcome::Swept(TemporalRetentionSweep {
            cutoff,
            attribute_history_chunks,
            scope_events,
            streams,
            temporal_entities,
            released_write_records,
        }))
    }

    /// Run one sweep delete in a transaction of its own, reporting how many
    /// rows it removed.
    async fn delete(&self, statement: &DeleteStatement) -> Result<u64, TemporalStoreError> {
        let mut transaction = self.pool.begin().await?;
        allow_unbounded_statements(&mut transaction).await?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let removed = sqlx::query_with(AssertSqlSafe(sql), values).execute(&mut *transaction).await?.rows_affected();
        transaction.commit().await?;
        Ok(removed)
    }

    /// Drop one hypertable's chunks that lie entirely before the cutoff.
    async fn drop_chunks(&self, hypertable: &str, cutoff: DateTime<Utc>) -> Result<usize, TemporalStoreError> {
        let mut transaction = self.pool.begin().await?;
        allow_unbounded_statements(&mut transaction).await?;
        let statement = Query::select().expr(drop_chunks(hypertable, Expr::val(cutoff))).to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let dropped = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&mut *transaction).await?.len();
        transaction.commit().await?;
        Ok(dropped)
    }
}

// `drop_chunks` removes whole chunks whose range ends before the cutoff, so the
// oldest surviving attribute row can be older than the cutoff by up to one
// chunk interval. That is the intended behaviour: retention ages attribute
// storage out at chunk granularity and never rewrites a chunk to trim rows out
// of it.

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

/// Delete the Scope events superseded at or before `cutoff` on every judged
/// axis: a later event of the same Entity falls at or before the cutoff on
/// `recorded_at`, and on `observed_at` unless the event has none.
///
/// The event a cutoff falls inside the validity of is the newest at or before
/// it, so no later event supersedes it there and it stays, together with every
/// row of its state, which shares its instant.
fn superseded_scope_events(cutoff: DateTime<Utc>) -> DeleteStatement {
    let mut statement = Query::delete();
    statement
        .from_table(ScopeHistory::Table)
        .and_where(Expr::col((ScopeHistory::Table, ScopeHistory::RecordedAt)).lte(cutoff));
    for axis in SUPERSEDED_AXES {
        let later = Query::select()
            .expr(Expr::val(1))
            .from_as(ScopeHistory::Table, Sn)
            .and_where(Expr::col((Sn, ScopeHistory::TemporalEntityKey)).equals((ScopeHistory::Table, ScopeHistory::TemporalEntityKey)))
            .and_where(Expr::col((Sn, axis)).gt(Expr::col((ScopeHistory::Table, axis))))
            .and_where(Expr::col((Sn, axis)).lte(cutoff))
            .to_owned();
        statement.cond_where(Condition::any().add(Expr::col((ScopeHistory::Table, axis)).is_null()).add(Expr::exists(later)));
    }
    statement
}

/// Delete the Scope events of a temporal entity no stream is left of, once
/// every one of them was recorded before `cutoff`.
///
/// A Scope only qualifies the attribute instances observed under it (annex C
/// clause C.5.16), so with the Entity's attribute history gone its Scope
/// history qualifies nothing; an Entity whose Scope changed after the cutoff
/// keeps it until that change is behind the horizon as well.
fn streamless_scope_events(cutoff: DateTime<Utc>) -> DeleteStatement {
    let stream = Query::select()
        .expr(Expr::val(1))
        .from_as(TemporalAttributeStreams::Table, S)
        .and_where(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((ScopeHistory::Table, ScopeHistory::TemporalEntityKey)))
        .to_owned();
    let recent = Query::select()
        .expr(Expr::val(1))
        .from_as(ScopeHistory::Table, Sn)
        .and_where(Expr::col((Sn, ScopeHistory::TemporalEntityKey)).equals((ScopeHistory::Table, ScopeHistory::TemporalEntityKey)))
        .and_where(Expr::col((Sn, ScopeHistory::RecordedAt)).gte(cutoff))
        .to_owned();
    Query::delete()
        .from_table(ScopeHistory::Table)
        .and_where(Expr::exists(stream).not())
        .and_where(Expr::exists(recent).not())
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
        streamless_scope_events,
        superseded_scope_events,
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
    fn attribute_history_is_aged_out_on_the_operational_write_clock() {
        let sql = Query::select()
            .expr(drop_chunks("attribute_history", Expr::val("2026-01-01T00:00:00Z")))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("drop_chunks('attribute_history', older_than => "), "{sql}");
        assert!(sql.contains("::timestamptz"), "{sql}");
        assert!(!sql.contains("observed_at"), "{sql}");
    }

    /// Clause 4.18: the Scope in force at the cutoff qualifies every instance
    /// kept after it, so an event goes only when a later one of its Entity
    /// falls at or before the cutoff on the write clock and on `observedAt`
    /// alike; the newest event at or before the cutoff on either axis stays.
    #[test]
    fn retention_keeps_the_scope_state_in_force_at_the_cutoff_on_every_axis() {
        let cutoff = now().expect("a fixed instant");
        let sql = superseded_scope_events(cutoff).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"DELETE FROM "scope_history" WHERE "scope_history"."recorded_at" <= '2026-08-11 12:00:00.000000 +00:00'"#),
            "{sql}"
        );
        for axis in ["recorded_at", "observed_at"] {
            assert!(
                sql.contains(&format!(
                    r#"EXISTS(SELECT 1 FROM "scope_history" AS "sn" WHERE "sn"."temporal_entity_key" = "scope_history"."temporal_entity_key" AND "sn"."{axis}" > "scope_history"."{axis}" AND "sn"."{axis}" <= '2026-08-11 12:00:00.000000 +00:00')"#
                )),
                "{axis}: {sql}"
            );
        }
        assert!(sql.contains(r#"("scope_history"."observed_at" IS NULL OR EXISTS("#), "{sql}");
        assert!(!sql.contains("drop_chunks"), "{sql}");
    }

    /// Annex C clause C.5.16: a Scope qualifies the instances observed under
    /// it, so an Entity left with no stream loses its Scope events once all
    /// of them are behind the cutoff.
    #[test]
    fn an_entity_whose_streams_are_all_gone_loses_its_scope_rows_too() {
        let cutoff = now().expect("a fixed instant");
        let sql = streamless_scope_events(cutoff).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(
                r#"NOT EXISTS(SELECT 1 FROM "temporal_attribute_streams" AS "s" WHERE "s"."temporal_entity_key" = "scope_history"."temporal_entity_key")"#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"NOT EXISTS(SELECT 1 FROM "scope_history" AS "sn" WHERE "sn"."temporal_entity_key" = "scope_history"."temporal_entity_key" AND "sn"."recorded_at" >= '2026-08-11 12:00:00.000000 +00:00')"#),
            "{sql}"
        );
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
        let cutoff = now().expect("a fixed instant");
        for sql in [
            collect_empty_streams().to_string(PostgresQueryBuilder),
            collect_empty_temporal_entities().to_string(PostgresQueryBuilder),
            superseded_scope_events(cutoff).to_string(PostgresQueryBuilder),
            streamless_scope_events(cutoff).to_string(PostgresQueryBuilder),
        ] {
            assert!(!sql.contains("INSERT"), "{sql}");
            assert!(!sql.contains("deleted_at"), "{sql}");
        }
    }
}
