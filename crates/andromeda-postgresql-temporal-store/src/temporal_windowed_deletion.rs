//! Windowed temporal history deletion (the `temporal_delete_by_query`
//! extension).
//!
//! A single `DELETE` over `attribute_history`, correlated to the streams the
//! request scopes through a `stream_key IN (subquery)` over
//! `temporal_attribute_streams` joined to `attribute_names` — the entity keys,
//! the attribute `drop`/`keep`, and the `datasetId` filter — with the time
//! window applied directly on the history row's axis column. The window bounds
//! are half-open and lower-inclusive, matching the temporal query's stage-1
//! semantics exactly (`Before` -> `< to`, `After` -> `>= from`, `Between` ->
//! `>= from AND < to`), so a delete removes exactly the instances a query over
//! the same window would return.
//!
//! Like [`crate::temporal_entity_deletion`], the hypertable carries no foreign
//! key back to the catalogue, so the sequence is stated here inside one
//! transaction: lock the entities, delete the history returning its write
//! identities, optionally remove the now-empty streams (only for an unbounded
//! `deleteAll`, the clause 5.6.13 equivalent), and release the idempotency
//! records the removed writes held.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    stored_catalog_error::StoredCatalogError,
    temporal_store_invariant::TemporalStoreInvariant,
    write_record_release::release_named,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    temporal::{
        history_delete::{AttributeDeleteScope, TemporalHistoryDeleteScope},
        window::{TemporalWindow, TimeInterval},
    },
};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{aggregate::count_all, branch::qualified};
use andromeda_postgresql_temporal_query::axis::axis_column;
use iri_rs::IriBuf;
use sea_query::{Condition, DeleteStatement, DynIden, Expr, ExprTrait, Iden, JoinType, LockType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Row};
use std::collections::BTreeSet;
use uuid::Uuid;

/// The `temporal_attribute_streams` alias in the correlated subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The `attribute_names` alias in the correlated subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
struct An;

/// The `temporal_entities` alias in the entity-key subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "te"]
struct Te;

/// The count of history instances the window and scope select.
#[derive(Iden, Clone, Copy)]
#[iden = "matched"]
struct Matched;

impl TemporalRepository {
    /// Count the history instances a windowed delete-by-query would remove.
    ///
    /// The same predicate the delete runs, under `COUNT`, so the bound check
    /// sees exactly what the delete would remove.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the count statement fails
    /// and [`TemporalStoreError::StoredCatalog`] when the returned count is
    /// outside the unsigned range a row count occupies.
    pub async fn count_windowed_history(
        &self,
        target: &ContextTarget,
        entity_ids: &[Identifier],
        scope: &TemporalHistoryDeleteScope,
    ) -> Result<u64, TemporalStoreError> {
        if entity_ids.is_empty() {
            return Ok(0);
        }
        let schema = schema_iden(target);
        let (sql, values) = windowed_count_statement(schema.as_ref(), entity_ids, scope).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(AssertSqlSafe(sql), values).fetch_one(&self.pool).await?;
        let matched: i64 = row.try_get("matched")?;
        Ok(u64::try_from(matched).map_err(|_| StoredCatalogError::NegativeCount { count: matched })?)
    }

    /// Delete the history instances a windowed delete-by-query selects and
    /// return how many were removed.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails and
    /// [`TemporalStoreInvariant::CountOutOfRange`] when the removed-row count
    /// is out of range.
    pub async fn delete_windowed_history(
        &self,
        target: &ContextTarget,
        entity_ids: &[Identifier],
        scope: &TemporalHistoryDeleteScope,
    ) -> Result<u64, TemporalStoreError> {
        if entity_ids.is_empty() {
            return Ok(0);
        }
        let mut tx = self.pool.begin().await?;
        let schema = schema_iden(target);

        // Lock the addressed entities so a concurrent temporal write cannot
        // interleave a new instant into the window mid-delete.
        let (lock_sql, lock_values) = lock_entities_statement(schema.as_ref(), entity_ids).build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(AssertSqlSafe(lock_sql), lock_values).execute(&mut *tx).await?;

        let (delete_sql, delete_values) = windowed_delete_statement(schema.as_ref(), entity_ids, scope).build_sqlx(PostgresQueryBuilder);
        let removed_rows = sqlx::query_with(AssertSqlSafe(delete_sql), delete_values).fetch_all(&mut *tx).await?;
        let removed = u64::try_from(removed_rows.len()).map_err(TemporalStoreInvariant::CountOutOfRange)?;

        let mut write_ids = BTreeSet::new();
        for row in &removed_rows {
            let write_id: Uuid = row.try_get("write_id")?;
            write_ids.insert(write_id);
        }

        // Only an unbounded `deleteAll` empties the scoped streams outright; a
        // bounded window removes a slice and the stream still holds instants
        // outside it, so its catalogue row stays (clause 5.6.13 equivalent).
        if scope.empties_streams() {
            let (cleanup_sql, cleanup_values) = empty_stream_cleanup_statement(schema.as_ref(), entity_ids, scope).build_sqlx(PostgresQueryBuilder);
            sqlx::query_with(AssertSqlSafe(cleanup_sql), cleanup_values).execute(&mut *tx).await?;
        }

        let write_ids = write_ids.into_iter().collect::<Vec<Uuid>>();
        release_named(&mut tx, &write_ids).await?;
        tx.commit().await?;
        Ok(removed)
    }
}

/// Lock the addressed temporal entities before any history is removed.
fn lock_entities_statement(schema: Option<&DynIden>, entity_ids: &[Identifier]) -> SelectStatement {
    Query::select()
        .column(TemporalEntities::TemporalEntityKey)
        .from(qualified(schema, TemporalEntities::Table))
        .and_where(Expr::col(TemporalEntities::Id).is_in(entity_ids.iter().map(Identifier::as_str)))
        .lock(LockType::Update)
        .to_owned()
}

/// The streams the request scopes: those of the addressed entities that carry a
/// targeted attribute and a selected `datasetId`.
fn matching_streams_subquery(schema: Option<&DynIden>, entity_ids: &[Identifier], scope: &TemporalHistoryDeleteScope) -> SelectStatement {
    let owning_entities = Query::select()
        .column((Te, TemporalEntities::TemporalEntityKey))
        .from(qualified(schema, TemporalEntities::Table).alias(Te))
        .and_where(Expr::col((Te, TemporalEntities::Id)).is_in(entity_ids.iter().map(Identifier::as_str)))
        .to_owned();

    let mut streams = Query::select();
    streams
        .column((S, TemporalAttributeStreams::StreamKey))
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(S))
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((S, TemporalAttributeStreams::AttributeNameKey)),
        )
        .and_where(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).in_subquery(owning_entities));

    match &scope.attributes {
        AttributeDeleteScope::All => {}
        AttributeDeleteScope::Drop(iris) => {
            streams.and_where(Expr::col((An, AttributeNames::Name)).is_in(iris.iter().map(IriBuf::as_str)));
        }
        AttributeDeleteScope::Keep(iris) => {
            streams.and_where(Expr::col((An, AttributeNames::Name)).is_not_in(iris.iter().map(IriBuf::as_str)));
        }
    }

    let dataset_column = Expr::col((S, TemporalAttributeStreams::DatasetId));
    match &scope.dataset {
        DatasetFilter::Unfiltered => {}
        DatasetFilter::DefaultOnly => {
            streams.and_where(dataset_column.is_null());
        }
        DatasetFilter::NamedOnly(named) => {
            streams.and_where(dataset_column.is_in(named.iter().map(ToString::to_string)));
        }
        DatasetFilter::DefaultOrNamed(named) => {
            streams.cond_where(
                Condition::any()
                    .add(Expr::col((S, TemporalAttributeStreams::DatasetId)).is_in(named.iter().map(ToString::to_string)))
                    .add(Expr::col((S, TemporalAttributeStreams::DatasetId)).is_null()),
            );
        }
    }

    streams.take()
}

/// The window predicate on the history row's axis column, half-open and
/// lower-inclusive to match the temporal query's stage-1 semantics exactly.
fn window_condition(window: &TemporalWindow) -> Condition {
    let axis = axis_column(window.timeproperty);
    let mut condition = Condition::all();
    match &window.interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => condition = condition.add(Expr::col(axis).lt(*instant)),
        TimeInterval::After(instant) => condition = condition.add(Expr::col(axis).gte(*instant)),
        TimeInterval::Between { from, to } => condition = condition.add(Expr::col(axis).gte(*from)).add(Expr::col(axis).lt(*to)),
    }
    condition
}

/// Count the history instances the scope and window select.
fn windowed_count_statement(schema: Option<&DynIden>, entity_ids: &[Identifier], scope: &TemporalHistoryDeleteScope) -> SelectStatement {
    Query::select()
        .expr_as(count_all(), Matched)
        .from(qualified(schema, AttributeHistory::Table))
        .and_where(Expr::col(AttributeHistory::StreamKey).in_subquery(matching_streams_subquery(schema, entity_ids, scope)))
        .cond_where(window_condition(&scope.window))
        .to_owned()
}

/// Delete the history instances the scope and window select, reporting each
/// removed row's write identity.
fn windowed_delete_statement(schema: Option<&DynIden>, entity_ids: &[Identifier], scope: &TemporalHistoryDeleteScope) -> DeleteStatement {
    Query::delete()
        .from_table(qualified(schema, AttributeHistory::Table))
        .and_where(Expr::col(AttributeHistory::StreamKey).in_subquery(matching_streams_subquery(schema, entity_ids, scope)))
        .cond_where(window_condition(&scope.window))
        .returning(Query::returning().column(AttributeHistory::WriteId))
        .to_owned()
}

/// Remove the scoped streams left empty by an unbounded `deleteAll`.
///
/// Guarded against any stream that still has a history row so a stream with a
/// surviving instant is never removed, which keeps the cleanup safe even if a
/// concurrent writer slipped a row in before the entity lock was taken.
fn empty_stream_cleanup_statement(schema: Option<&DynIden>, entity_ids: &[Identifier], scope: &TemporalHistoryDeleteScope) -> DeleteStatement {
    let streams_with_history = Query::select()
        .column(AttributeHistory::StreamKey)
        .from(qualified(schema, AttributeHistory::Table))
        .to_owned();
    Query::delete()
        .from_table(qualified(schema, TemporalAttributeStreams::Table))
        .and_where(Expr::col(TemporalAttributeStreams::StreamKey).in_subquery(matching_streams_subquery(schema, entity_ids, scope)))
        .and_where(Expr::col(TemporalAttributeStreams::StreamKey).not_in_subquery(streams_with_history))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::temporal_windowed_deletion::{empty_stream_cleanup_statement, windowed_count_statement, windowed_delete_statement};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        dataset::DatasetFilter,
        temporal::{
            history_delete::{AttributeDeleteScope, TemporalHistoryDeleteScope},
            window::{TemporalWindow, TimeInterval},
        },
    };
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use sea_query::PostgresQueryBuilder;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    fn ids() -> Vec<Identifier> {
        vec!["urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")]
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn scope(attributes: AttributeDeleteScope, dataset: DatasetFilter, interval: TimeInterval, delete_all: bool) -> TemporalHistoryDeleteScope {
        TemporalHistoryDeleteScope {
            attributes,
            dataset,
            window: TemporalWindow::builder().interval(interval).timeproperty(TimeProp::Observed).build(),
            delete_all,
        }
    }

    fn delete_sql(scope: &TemporalHistoryDeleteScope) -> String {
        windowed_delete_statement(None, &ids(), scope).to_string(PostgresQueryBuilder)
    }

    /// Clause 4.11 defaults the axis to `observedAt`; each time property filters
    /// its own column with no `COALESCE`, matching stage 1.
    #[test]
    fn each_time_property_filters_its_own_column() {
        let expected = [
            (TimeProp::Observed, "\"observed_at\""),
            (TimeProp::Created, "\"created_at\""),
            (TimeProp::Modified, "\"modified_at\""),
            (TimeProp::Deleted, "\"deleted_at\""),
        ];
        for (prop, column) in expected {
            let mut scope = scope(
                AttributeDeleteScope::All,
                DatasetFilter::Unfiltered,
                TimeInterval::After(instant("2024-01-01T00:00:00Z")),
                false,
            );
            scope.window = TemporalWindow::builder().interval(scope.window.interval).timeproperty(prop).build();
            let sql = delete_sql(&scope);
            assert!(sql.contains(column), "{prop}: {sql}");
            assert!(!sql.contains("COALESCE"), "{prop}: {sql}");
        }
    }

    /// Window inclusivity matches stage 1: `After` is `>=`, `Before` is `<`,
    /// `Between` is `>=` and `<`.
    #[test]
    fn the_window_bounds_are_half_open_and_lower_inclusive() {
        let after = delete_sql(&scope(
            AttributeDeleteScope::All,
            DatasetFilter::Unfiltered,
            TimeInterval::After(instant("2024-01-01T00:00:00Z")),
            false,
        ));
        assert!(after.contains(">="), "{after}");
        let before = delete_sql(&scope(
            AttributeDeleteScope::All,
            DatasetFilter::Unfiltered,
            TimeInterval::Before(instant("2024-01-01T00:00:00Z")),
            false,
        ));
        assert!(before.contains('<') && !before.contains("<="), "{before}");
        let between = delete_sql(&scope(
            AttributeDeleteScope::All,
            DatasetFilter::Unfiltered,
            TimeInterval::Between {
                from: instant("2024-01-01T00:00:00Z"),
                to: instant("2024-06-01T00:00:00Z"),
            },
            false,
        ));
        assert!(between.contains(">="), "{between}");
        assert!(between.contains('<'), "{between}");
    }

    /// `drop` renders `IN`, `keep` renders `NOT IN`, and neither is absent.
    #[test]
    fn the_attribute_scope_renders_in_not_in_or_nothing() {
        let drop = delete_sql(&scope(
            AttributeDeleteScope::Drop(vec![iri("https://example.com/speed")]),
            DatasetFilter::Unfiltered,
            TimeInterval::Unbounded,
            false,
        ));
        assert!(drop.contains("\"an\".\"name\" IN"), "{drop}");
        let keep = delete_sql(&scope(
            AttributeDeleteScope::Keep(vec![iri("https://example.com/speed")]),
            DatasetFilter::Unfiltered,
            TimeInterval::Unbounded,
            false,
        ));
        assert!(keep.contains("\"an\".\"name\" NOT IN"), "{keep}");
        let all = delete_sql(&scope(AttributeDeleteScope::All, DatasetFilter::Unfiltered, TimeInterval::Unbounded, false));
        assert!(!all.contains("\"an\".\"name\""), "{all}");
    }

    /// The dataset filter renders `IS NULL`, `IN`, an `OR` of both, or nothing.
    #[test]
    fn the_dataset_filter_renders_its_predicate() {
        let default_only = delete_sql(&scope(AttributeDeleteScope::All, DatasetFilter::DefaultOnly, TimeInterval::Unbounded, false));
        assert!(default_only.contains("\"dataset_id\" IS NULL"), "{default_only}");
        let named = delete_sql(&scope(
            AttributeDeleteScope::All,
            DatasetFilter::NamedOnly(vec!["urn:ngsi-ld:Dataset:1".parse().expect("valid URI")]),
            TimeInterval::Unbounded,
            false,
        ));
        assert!(named.contains("\"dataset_id\" IN"), "{named}");
        let both = delete_sql(&scope(
            AttributeDeleteScope::All,
            DatasetFilter::DefaultOrNamed(vec!["urn:ngsi-ld:Dataset:1".parse().expect("valid URI")]),
            TimeInterval::Unbounded,
            false,
        ));
        assert!(both.contains("OR") && both.contains("IS NULL"), "{both}");
        let unfiltered = delete_sql(&scope(AttributeDeleteScope::All, DatasetFilter::Unfiltered, TimeInterval::Unbounded, false));
        assert!(!unfiltered.contains("\"dataset_id\""), "{unfiltered}");
    }

    /// The delete correlates through a `stream_key IN (subquery)` and reports
    /// each removed row's write identity.
    #[test]
    fn the_delete_is_a_correlated_subquery_returning_write_ids() {
        let sql = delete_sql(&scope(AttributeDeleteScope::All, DatasetFilter::Unfiltered, TimeInterval::Unbounded, false));
        assert!(sql.starts_with("DELETE FROM \"attribute_history\""), "{sql}");
        assert!(sql.contains("\"stream_key\" IN (SELECT"), "{sql}");
        assert!(sql.contains("RETURNING \"write_id\""), "{sql}");
    }

    /// The count carries the same scope predicate under `COUNT(*)`.
    #[test]
    fn the_count_carries_the_same_predicate() {
        let count = windowed_count_statement(
            None,
            &ids(),
            &scope(AttributeDeleteScope::All, DatasetFilter::Unfiltered, TimeInterval::Unbounded, false),
        )
        .to_string(PostgresQueryBuilder);
        assert!(count.contains("COUNT(*)"), "{count}");
        assert!(count.contains("\"stream_key\" IN (SELECT"), "{count}");
    }

    /// Empty-stream cleanup removes only streams with no surviving history.
    #[test]
    fn empty_stream_cleanup_guards_on_surviving_history() {
        let sql = empty_stream_cleanup_statement(
            None,
            &ids(),
            &scope(AttributeDeleteScope::All, DatasetFilter::Unfiltered, TimeInterval::Unbounded, true),
        )
        .to_string(PostgresQueryBuilder);
        assert!(sql.starts_with("DELETE FROM \"temporal_attribute_streams\""), "{sql}");
        assert!(sql.contains("\"stream_key\" NOT IN (SELECT"), "{sql}");
    }
}
