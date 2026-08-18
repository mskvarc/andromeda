//! The statements over the rows a snapshot captured: the bulk write of the
//! materialised documents, their copy under a cloned snapshot, and the reads
//! that page through or fetch one of them.
//!
//! Both child tables - `snapshot_entities` and `snapshot_temporal_entities` -
//! share one shape, so every statement here is built for a caller-named table
//! and document column. They are keyed by the snapshot's surrogate, which each
//! statement resolves from the URI it is handed through
//! [`andromeda_postgresql_context_query::snapshot_key::snapshot_key_of`]: the
//! surrogate never crosses this boundary.

use crate::snapshot::{params::SnapshotQueryParams, persistence_error};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_context_query::{
    snapshot_key::snapshot_key_of,
    snapshot_listing::{SnapshotListing, SnapshotListingParams, build_get_row_query, build_listing_query},
};
use andromeda_postgresql_schema::snapshot::SnapshotEntities;
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use andromeda_snapshot::{error::SnapshotError, store::SnapshotEntityInput};
use sea_query::{Alias, Expr, ExprTrait, FunctionCall, Iden, IntoIden, OnConflict, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{PgConnection, PgPool, Row};

/// The `UNNEST` staging alias, `d`, a batch of snapshot rows is expanded under.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The `UNNEST($2::text[], $3::jsonb[])` staging source a batch of snapshot rows
/// expands through.
fn snapshot_rows_unnest(columns: &[UnnestColumn; 2]) -> FunctionCall {
    unnest_call([(&columns[0], external_placeholder(2)), (&columns[1], external_placeholder(3))])
}

/// The bulk snapshot-row upsert for one backing table: the snapshot's URI bound
/// as `$1` and resolved to its surrogate in the statement, the entity ids and
/// documents bound by the caller's chain as `$2`/`$3`.
pub(crate) fn batch_insert_sql(table: impl IntoIden + Copy + 'static, data_column: impl IntoIden + Copy + 'static) -> Result<String, SnapshotError> {
    let columns = [
        UnnestColumn::new(SnapshotEntities::EntityId, PostgresType::TextArray),
        UnnestColumn::new(data_column, PostgresType::JsonbArray),
    ];
    let call = snapshot_rows_unnest(&columns);

    let mut select = Query::select();
    select
        .expr(snapshot_key_of(external_placeholder(1)))
        .expr(Expr::col((D, SnapshotEntities::EntityId)))
        .expr(Expr::col((D, data_column.into_iden())))
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(table).columns([
        SnapshotEntities::SnapshotKey.into_iden(),
        SnapshotEntities::EntityId.into_iden(),
        data_column.into_iden(),
    ]);
    insert.select_from(select).map_err(persistence_error)?.on_conflict(
        OnConflict::columns([SnapshotEntities::SnapshotKey, SnapshotEntities::EntityId])
            .update_column(data_column)
            .to_owned(),
    );

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// Copy the captured rows of one snapshot `$1` under a new snapshot `$2`, both
/// named by URI and resolved to their surrogates in the statement.
pub(crate) fn clone_rows_sql(
    table: impl IntoIden + Copy + 'static,
    entity_id: impl IntoIden + Copy + 'static,
    data_column: impl IntoIden + Copy + 'static,
) -> Result<String, SnapshotError> {
    let mut select = Query::select();
    select
        .expr(snapshot_key_of(external_placeholder(2)))
        .column(entity_id)
        .column(data_column)
        .from(table)
        .and_where(Expr::col(SnapshotEntities::SnapshotKey).eq(snapshot_key_of(external_placeholder(1))));
    let select = select;

    let mut insert = Query::insert();
    insert
        .into_table(table)
        .columns([SnapshotEntities::SnapshotKey.into_iden(), entity_id.into_iden(), data_column.into_iden()]);
    insert.select_from(select).map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Bind and run one bulk snapshot-row insert.
pub(crate) async fn run_batch_insert(pool: &PgPool, sql: &str, snapshot_id: &Uri, entities: &[SnapshotEntityInput<'_>]) -> Result<(), SnapshotError> {
    if entities.is_empty() {
        return Ok(());
    }
    let entity_ids: Vec<&str> = entities.iter().map(|entity| entity.entity_id.as_str()).collect();
    let payloads: Vec<&Value> = entities.iter().map(|e| e.data).collect();
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(snapshot_id.as_str())
        .bind(&entity_ids)
        .bind(&payloads)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// Bind and run one captured-row copy from `source_id` to `new_id`.
pub(crate) async fn run_clone_rows(connection: &mut PgConnection, sql: &str, source_id: &Uri, new_id: &Uri) -> Result<(), SnapshotError> {
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(source_id.as_str())
        .bind(new_id.as_str())
        .execute(connection)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// Retrieve one prepared snapshot row by snapshot and entity identifiers.
pub(crate) async fn get_snapshot_row(
    pool: &PgPool,
    table: Alias,
    data_column: Alias,
    snapshot_id: &Uri,
    entity_id: &Uri,
) -> Result<Option<Value>, SnapshotError> {
    let data_name = Iden::to_string(&data_column);
    let (sql, values) = build_get_row_query(table, data_column, snapshot_id, entity_id).build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;

    match row {
        Some(row) => {
            let data: Value = row.try_get(data_name.as_str()).map_err(persistence_error)?;
            Ok(Some(data))
        }
        None => Ok(None),
    }
}

/// Query one snapshot backing table with the common selector and pagination contract.
pub(crate) async fn query_snapshot_table(params: &SnapshotQueryParams<'_>) -> Result<(Vec<Value>, i64), SnapshotError> {
    let SnapshotListing { data, count } = build_listing_query(&SnapshotListingParams {
        table: params.table,
        data_column: params.data_column,
        snapshot_id: params.snapshot_id,
        entity_type: params.entity_type,
        entity_ids: params.entity_ids,
        limit: params.limit,
        offset: params.offset,
    });

    let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
    let count_row = sqlx::query_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
        .fetch_one(params.pool)
        .await
        .map_err(persistence_error)?;
    let total: i64 = count_row.try_get("cnt").map_err(persistence_error)?;

    let (data_sql, data_values) = data.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(data_sql.as_str()), data_values)
        .fetch_all(params.pool)
        .await
        .map_err(persistence_error)?;
    let entities: Vec<Value> = rows
        .iter()
        .map(|r| r.try_get(params.data_column))
        .collect::<Result<Vec<_>, _>>()
        .map_err(persistence_error)?;

    Ok((entities, total))
}

#[cfg(test)]
mod tests {
    use crate::snapshot::content::{batch_insert_sql, clone_rows_sql};
    use andromeda_postgresql_schema::snapshot::{SnapshotEntities, SnapshotTemporalEntities};

    /// The surrogate lookup every child-table statement resolves a URI through.
    const KEY_OF_1: &str = r#"(SELECT "snapshots"."snapshot_key" FROM "snapshots" WHERE "snapshots"."id" = ($1))"#;

    #[test]
    fn a_batch_insert_resolves_the_surrogate_once_and_upserts_on_the_snapshot_and_entity_grain() {
        let entities = batch_insert_sql(SnapshotEntities::Table, SnapshotEntities::EntityData).expect("builds");
        assert!(
            entities.starts_with(r#"INSERT INTO "snapshot_entities" ("snapshot_key", "entity_id", "entity_data") SELECT "#),
            "{entities}"
        );
        assert!(entities.contains(KEY_OF_1), "{entities}");
        assert!(
            entities.contains(r#"ON CONFLICT ("snapshot_key", "entity_id") DO UPDATE SET "entity_data" = "excluded"."entity_data""#),
            "{entities}"
        );
        assert!(entities.contains("AS d(entity_id, entity_data)"), "{entities}");
        let temporal = batch_insert_sql(SnapshotTemporalEntities::Table, SnapshotTemporalEntities::TemporalData).expect("builds");
        assert!(temporal.contains(r#"INSERT INTO "snapshot_temporal_entities""#), "{temporal}");
        assert!(temporal.contains(r#""temporal_data" = "excluded"."temporal_data""#), "{temporal}");
    }

    #[test]
    fn a_row_clone_resolves_both_surrogates_from_their_identifiers() {
        let sql = clone_rows_sql(SnapshotEntities::Table, SnapshotEntities::EntityId, SnapshotEntities::EntityData).expect("builds");
        assert!(
            sql.starts_with(r#"INSERT INTO "snapshot_entities" ("snapshot_key", "entity_id", "entity_data") SELECT "#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(SELECT "snapshots"."snapshot_key" FROM "snapshots" WHERE "snapshots"."id" = ($2))"#),
            "{sql}"
        );
        assert!(sql.contains(&format!(r#"WHERE "snapshot_key" = {KEY_OF_1}"#)), "{sql}");
    }
}
