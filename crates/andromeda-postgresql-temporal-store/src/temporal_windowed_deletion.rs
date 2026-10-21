//! Windowed temporal history deletion (the `temporal_delete_by_query`
//! extension).
//!
//! The streams the request scopes are resolved first, from
//! `temporal_attribute_streams` joined to `attribute_names` (the entity keys,
//! the attribute `drop`/`keep`, and the `datasetId` filter), and the `DELETE`
//! over `attribute_history` binds them as values, with the time window applied
//! directly on the history row's axis column: on a converted chunk TimescaleDB
//! narrows the compressed batches a statement decompresses only by bound values
//! on the segmenting stream key and the axis column's min/max summary, and a
//! subquery would have it decompress every batch of the chunk. The window bounds
//! are half-open and lower-inclusive, matching the temporal query's stage-1
//! semantics exactly (`Before` -> `< to`, `After` -> `>= from`, `Between` ->
//! `>= from AND < to`), so a delete removes exactly the instances a query over
//! the same window would return.
//!
//! Like [`crate::temporal_entity_deletion`], the hypertable carries no foreign
//! key back to the catalogue, so the sequence is stated here inside one
//! transaction: lock the entities, resolve the streams, read the write
//! identities of the rows in the window, delete them, optionally remove the
//! now-empty streams (only for an unbounded `deleteAll`, the clause 5.6.13
//! equivalent), prune the shapes no instance left in a stream carries, and
//! release the idempotency records the removed writes held.
//! A window that cuts through a compressed batch has that batch decompressed,
//! in proportion to what the request removes, so the transaction lifts the
//! decompression limit for itself.

use crate::{
    decompression_allowance::allow_confined_decompression,
    error::TemporalStoreError,
    history_removal::stream_key_array,
    repo::TemporalRepository,
    stored_catalog_error::StoredCatalogError,
    stream_shape_pruning::prune_stream_shapes,
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
use andromeda_postgresql_seaquery::{aggregate::count_all, branch::qualified, pg_array::any};
use andromeda_postgresql_temporal_query::axis::axis_column;
use iri_rs::IriBuf;
use sea_query::{Condition, DeleteStatement, DynIden, Expr, ExprTrait, Iden, JoinType, LockType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgConnection, Row};
use uuid::Uuid;

/// The `temporal_attribute_streams` alias in the correlated subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The `attribute_names` alias in the correlated subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
struct An;

/// The `attribute_history` alias of the surviving-history probe.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
struct Ah;

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
        let mut connection = self.pool.acquire().await?;
        let stream_keys = matching_stream_keys(&mut connection, schema.as_ref(), entity_ids, scope).await?;
        if stream_keys.is_empty() {
            return Ok(0);
        }
        let (sql, values) = windowed_count_statement(schema.as_ref(), &stream_keys, &scope.window).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(AssertSqlSafe(sql), values).fetch_one(&mut *connection).await?;
        let matched: i64 = row.try_get("matched")?;
        Ok(u64::try_from(matched).map_err(|_| StoredCatalogError::NegativeCount { count: matched })?)
    }

    /// Delete the history instances a windowed delete-by-query selects and
    /// return how many were removed.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails.
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

        let stream_keys = matching_stream_keys(&mut tx, schema.as_ref(), entity_ids, scope).await?;
        if stream_keys.is_empty() {
            tx.rollback().await?;
            return Ok(0);
        }
        allow_confined_decompression(&mut tx).await?;
        let (writes_sql, writes_values) = windowed_writes_statement(schema.as_ref(), &stream_keys, &scope.window).build_sqlx(PostgresQueryBuilder);
        let write_ids: Vec<Uuid> = sqlx::query_scalar_with(AssertSqlSafe(writes_sql), writes_values).fetch_all(&mut *tx).await?;
        let (delete_sql, delete_values) = windowed_delete_statement(schema.as_ref(), &stream_keys, &scope.window).build_sqlx(PostgresQueryBuilder);
        let removed = sqlx::query_with(AssertSqlSafe(delete_sql), delete_values)
            .execute(&mut *tx)
            .await?
            .rows_affected();

        // Only an unbounded `deleteAll` empties the scoped streams outright; a
        // bounded window removes a slice and the stream still holds instants
        // outside it, so its catalogue row stays (clause 5.6.13 equivalent).
        if scope.empties_streams() {
            let (cleanup_sql, cleanup_values) = empty_stream_cleanup_statement(schema.as_ref(), &stream_keys).build_sqlx(PostgresQueryBuilder);
            sqlx::query_with(AssertSqlSafe(cleanup_sql), cleanup_values).execute(&mut *tx).await?;
        }
        prune_stream_shapes(&mut tx, schema.as_ref(), &stream_keys).await?;

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

/// The keys of the streams the request scopes.
async fn matching_stream_keys(
    connection: &mut PgConnection,
    schema: Option<&DynIden>,
    entity_ids: &[Identifier],
    scope: &TemporalHistoryDeleteScope,
) -> Result<Vec<i64>, TemporalStoreError> {
    let (sql, values) = matching_streams_query(schema, entity_ids, scope).build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_all(connection).await?)
}

/// The streams the request scopes: those of the addressed entities that carry a
/// targeted attribute and a selected `datasetId`.
fn matching_streams_query(schema: Option<&DynIden>, entity_ids: &[Identifier], scope: &TemporalHistoryDeleteScope) -> SelectStatement {
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

/// `stream_key = ANY(keys)` and the window: the history rows the request
/// removes.
fn windowed_rows(stream_keys: &[i64], window: &TemporalWindow) -> Condition {
    Condition::all()
        .add(Expr::col(AttributeHistory::StreamKey).eq(any(stream_key_array(stream_keys))))
        .add(window_condition(window))
}

/// Count the history instances the streams and window select.
fn windowed_count_statement(schema: Option<&DynIden>, stream_keys: &[i64], window: &TemporalWindow) -> SelectStatement {
    Query::select()
        .expr_as(count_all(), Matched)
        .from(qualified(schema, AttributeHistory::Table))
        .cond_where(windowed_rows(stream_keys, window))
        .to_owned()
}

/// The distinct write identities of the history instances the streams and
/// window select, read before they are removed.
fn windowed_writes_statement(schema: Option<&DynIden>, stream_keys: &[i64], window: &TemporalWindow) -> SelectStatement {
    Query::select()
        .distinct()
        .column(AttributeHistory::WriteId)
        .from(qualified(schema, AttributeHistory::Table))
        .cond_where(windowed_rows(stream_keys, window))
        .to_owned()
}

/// Delete the history instances the streams and window select.
fn windowed_delete_statement(schema: Option<&DynIden>, stream_keys: &[i64], window: &TemporalWindow) -> DeleteStatement {
    Query::delete()
        .from_table(qualified(schema, AttributeHistory::Table))
        .cond_where(windowed_rows(stream_keys, window))
        .to_owned()
}

/// Remove the scoped streams left empty by an unbounded `deleteAll`.
///
/// Guarded against any stream that still has a history row so a stream with a
/// surviving instant is never removed, which keeps the cleanup safe even if a
/// concurrent writer slipped a row in before the entity lock was taken. The
/// guard binds the resolved streams inside the probe as well: correlated
/// alone, the planner reads every history row of the relation to hash it.
fn empty_stream_cleanup_statement(schema: Option<&DynIden>, stream_keys: &[i64]) -> DeleteStatement {
    let surviving_history = Query::select()
        .expr(Expr::val(1))
        .from(qualified(schema, AttributeHistory::Table).alias(Ah))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).eq(any(stream_key_array(stream_keys))))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)))
        .to_owned();
    Query::delete()
        .from_table(qualified(schema, TemporalAttributeStreams::Table))
        .and_where(Expr::col(TemporalAttributeStreams::StreamKey).eq(any(stream_key_array(stream_keys))))
        .and_where(Expr::exists(surviving_history).not())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::temporal_windowed_deletion::{
        empty_stream_cleanup_statement,
        matching_streams_query,
        windowed_count_statement,
        windowed_delete_statement,
        windowed_writes_statement,
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        dataset::DatasetFilter,
        temporal::{
            history_delete::{AttributeDeleteScope, TemporalHistoryDeleteScope},
            window::{TemporalWindow, TimeInterval},
        },
    };
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("valid instant").get()
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
        windowed_delete_statement(None, &[1, 2], &scope.window).to_string(PostgresQueryBuilder)
    }

    fn streams_sql(scope: &TemporalHistoryDeleteScope) -> String {
        matching_streams_query(None, &ids(), scope).to_string(PostgresQueryBuilder)
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
        let drop = streams_sql(&scope(
            AttributeDeleteScope::Drop(vec![iri("https://example.com/speed")]),
            DatasetFilter::Unfiltered,
            TimeInterval::Unbounded,
            false,
        ));
        assert!(drop.contains("\"an\".\"name\" IN"), "{drop}");
        let keep = streams_sql(&scope(
            AttributeDeleteScope::Keep(vec![iri("https://example.com/speed")]),
            DatasetFilter::Unfiltered,
            TimeInterval::Unbounded,
            false,
        ));
        assert!(keep.contains("\"an\".\"name\" NOT IN"), "{keep}");
        let all = streams_sql(&scope(AttributeDeleteScope::All, DatasetFilter::Unfiltered, TimeInterval::Unbounded, false));
        assert!(!all.contains("\"an\".\"name\""), "{all}");
    }

    /// The dataset filter renders `IS NULL`, `IN`, an `OR` of both, or nothing.
    #[test]
    fn the_dataset_filter_renders_its_predicate() {
        let default_only = streams_sql(&scope(AttributeDeleteScope::All, DatasetFilter::DefaultOnly, TimeInterval::Unbounded, false));
        assert!(default_only.contains("\"dataset_id\" IS NULL"), "{default_only}");
        let named = streams_sql(&scope(
            AttributeDeleteScope::All,
            DatasetFilter::NamedOnly(vec!["urn:ngsi-ld:Dataset:1".parse().expect("valid URI")]),
            TimeInterval::Unbounded,
            false,
        ));
        assert!(named.contains("\"dataset_id\" IN"), "{named}");
        let both = streams_sql(&scope(
            AttributeDeleteScope::All,
            DatasetFilter::DefaultOrNamed(vec!["urn:ngsi-ld:Dataset:1".parse().expect("valid URI")]),
            TimeInterval::Unbounded,
            false,
        ));
        assert!(both.contains("OR") && both.contains("IS NULL"), "{both}");
        let unfiltered = streams_sql(&scope(AttributeDeleteScope::All, DatasetFilter::Unfiltered, TimeInterval::Unbounded, false));
        assert!(!unfiltered.contains("\"dataset_id\""), "{unfiltered}");
    }

    #[test]
    fn the_delete_binds_the_resolved_streams_as_values_and_returns_nothing() {
        let sql = delete_sql(&scope(
            AttributeDeleteScope::All,
            DatasetFilter::Unfiltered,
            TimeInterval::After(instant("2024-01-01T00:00:00Z")),
            false,
        ));
        assert_eq!(
            sql,
            r#"DELETE FROM "attribute_history" WHERE "stream_key" = ANY(ARRAY [1,2]) AND "observed_at" >= '2024-01-01 00:00:00.000000 +00:00'"#
        );
    }

    #[test]
    fn the_count_and_the_write_identities_read_exactly_the_rows_the_delete_removes() {
        let window = scope(
            AttributeDeleteScope::All,
            DatasetFilter::Unfiltered,
            TimeInterval::Before(instant("2024-01-01T00:00:00Z")),
            false,
        )
        .window;
        let predicate = r#"WHERE "stream_key" = ANY(ARRAY [3]) AND "observed_at" < '2024-01-01 00:00:00.000000 +00:00'"#;
        let count = windowed_count_statement(None, &[3], &window).to_string(PostgresQueryBuilder);
        let writes = windowed_writes_statement(None, &[3], &window).to_string(PostgresQueryBuilder);
        let delete = windowed_delete_statement(None, &[3], &window).to_string(PostgresQueryBuilder);
        assert!(count.starts_with(r#"SELECT COUNT(*) AS "matched""#) && count.ends_with(predicate), "{count}");
        assert!(writes.starts_with(r#"SELECT DISTINCT "write_id""#) && writes.ends_with(predicate), "{writes}");
        assert!(delete.ends_with(predicate), "{delete}");
    }

    #[test]
    fn empty_stream_cleanup_probes_the_surviving_history_of_each_resolved_stream() {
        let sql = empty_stream_cleanup_statement(None, &[4]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"DELETE FROM "temporal_attribute_streams" WHERE "stream_key" = ANY(ARRAY [4]) AND (NOT EXISTS(SELECT 1 FROM "attribute_history" AS "ah" WHERE "ah"."stream_key" = ANY(ARRAY [4]) AND "ah"."stream_key" = "temporal_attribute_streams"."stream_key"))"#
        );
    }
}
