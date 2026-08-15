//! `SnapshotStore` trait implementation for `SnapshotRepository`.

use crate::snapshot::{mapper::row_to_snapshot, params::SnapshotQueryParams, repo::SnapshotRepository};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_postgresql_context_query::{
    snapshot_listing::{SnapshotListing, SnapshotListingParams, build_get_row_query, build_listing_query},
    snapshot_purge::build_match_query,
};
use andromeda_postgresql_schema::snapshot::{SnapshotEntities, SnapshotTemporalEntities, Snapshots};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    clock::now,
    jsonb::jsonb_object_keys,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use andromeda_snapshot::{
    error::SnapshotError,
    store::{InsertSnapshotParams, SnapshotAttributeInfo, SnapshotEntityInput, SnapshotStore, SnapshotTypeInfo},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::{
    Alias,
    Expr,
    ExprTrait,
    Func,
    FunctionCall,
    Iden,
    IntoIden,
    Keyword,
    OnConflict,
    Order,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    SimpleExpr,
};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{Row, postgres::PgPool};
use std::fmt::Display;
use url::Url;

/// The `t` output column of a distinct-type read.
#[derive(Iden, Clone, Copy)]
#[iden = "t"]
struct TypeColumn;

/// The `cnt` output column of a count read.
#[derive(Iden, Clone, Copy)]
#[iden = "cnt"]
struct CountColumn;

/// The `key` set-returning source and its single output column.
#[derive(Iden, Clone, Copy)]
#[iden = "key"]
struct KeyColumn;

/// The keyword members `jsonb_object_keys` yields that are not attributes.
const NON_ATTRIBUTE_KEYS: [&str; 3] = ["id", "type", "@context"];

/// Erase an adapter failure into the snapshot policy's persistence category.
fn persistence_error(error: impl Display) -> SnapshotError {
    SnapshotError::persistence(error.to_string())
}

/// `entity_data ->> 'type'` - the entity's type token as text.
fn entity_type_text(table: impl IntoIden) -> SimpleExpr {
    SimpleExpr::from(Expr::col((table.into_iden(), SnapshotEntities::EntityData))).json_get_text(Expr::val("type"))
}

/// The stored-snapshot projection every read shares, decoding the status enum to
/// text under the column name the mapper reads it by.
fn apply_snapshot_projection(select: &mut SelectStatement) {
    select
        .column(Snapshots::Id)
        .column(Snapshots::SnapshotQueries)
        .column(Snapshots::SnapshotTemporalQueries)
        .column(Snapshots::SnapshotLifetime)
        .column(Snapshots::SnapshotPriority)
        .column(Snapshots::Endpoint)
        .column(Snapshots::ReceiverInfo)
        .expr_as(cast(Expr::col(Snapshots::Status), PostgresType::Text), Snapshots::Status)
        .column(Snapshots::QueriesDetails)
        .column(Snapshots::TemporalQueriesDetails)
        .column(Snapshots::CreatedAt)
        .column(Snapshots::ModifiedAt)
        .column(Snapshots::ExpiresAt)
        .column(Snapshots::LastUsedAt);
}

/// A projecting select of `snapshots`, ready for a caller's `WHERE`.
fn snapshot_select() -> SelectStatement {
    let mut select = Query::select();
    apply_snapshot_projection(&mut select);
    select.from(Snapshots::Table);
    select
}

/// The snapshot insert, its fourteen values bound by the caller's chain; the
/// status marker is cast to the `snapshot_status` enum.
fn snapshot_create_sql() -> Result<String, SnapshotError> {
    let mut insert = Query::insert();
    insert.into_table(Snapshots::Table).columns([
        Snapshots::Id,
        Snapshots::SnapshotQueries,
        Snapshots::SnapshotTemporalQueries,
        Snapshots::SnapshotLifetime,
        Snapshots::SnapshotPriority,
        Snapshots::Endpoint,
        Snapshots::ReceiverInfo,
        Snapshots::Status,
        Snapshots::QueriesDetails,
        Snapshots::TemporalQueriesDetails,
        Snapshots::CreatedAt,
        Snapshots::ModifiedAt,
        Snapshots::ExpiresAt,
        Snapshots::LastUsedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            cast(external_placeholder(8), PostgresType::SnapshotStatus),
            external_placeholder(9),
            external_placeholder(10),
            external_placeholder(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Copy the control record of `$1` into a new snapshot `$2` expiring at `$3`.
fn clone_snapshot_sql() -> Result<String, SnapshotError> {
    let mut select = Query::select();
    select
        .expr(external_placeholder(2))
        .column(Snapshots::SnapshotQueries)
        .column(Snapshots::SnapshotTemporalQueries)
        .column(Snapshots::SnapshotLifetime)
        .column(Snapshots::SnapshotPriority)
        .column(Snapshots::Endpoint)
        .column(Snapshots::ReceiverInfo)
        .column(Snapshots::Status)
        .column(Snapshots::QueriesDetails)
        .column(Snapshots::TemporalQueriesDetails)
        .expr(now())
        .expr(now())
        .expr(external_placeholder(3))
        .expr(SimpleExpr::Keyword(Keyword::Null))
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(external_placeholder(1)));
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(Snapshots::Table).columns([
        Snapshots::Id,
        Snapshots::SnapshotQueries,
        Snapshots::SnapshotTemporalQueries,
        Snapshots::SnapshotLifetime,
        Snapshots::SnapshotPriority,
        Snapshots::Endpoint,
        Snapshots::ReceiverInfo,
        Snapshots::Status,
        Snapshots::QueriesDetails,
        Snapshots::TemporalQueriesDetails,
        Snapshots::CreatedAt,
        Snapshots::ModifiedAt,
        Snapshots::ExpiresAt,
        Snapshots::LastUsedAt,
    ]);
    insert.select_from(select).map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// Copy the entity rows of one snapshot `$1` into a new snapshot `$2`.
fn clone_rows_sql(
    table: impl IntoIden + Copy + 'static,
    entity_id: impl IntoIden + Copy + 'static,
    data_column: impl IntoIden + Copy + 'static,
) -> Result<String, SnapshotError> {
    let mut select = Query::select();
    select
        .expr(external_placeholder(2))
        .column(entity_id)
        .column(data_column)
        .from(table)
        .and_where(Expr::col(SnapshotEntities::SnapshotId).eq(external_placeholder(1)));
    let select = select;

    let mut insert = Query::insert();
    insert
        .into_table(table)
        .columns([SnapshotEntities::SnapshotId.into_iden(), entity_id.into_iden(), data_column.into_iden()]);
    insert.select_from(select).map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The `UNNEST($2::text[], $3::jsonb[])` staging source a batch of snapshot rows
/// expands through.
fn snapshot_rows_unnest(columns: &[UnnestColumn; 2]) -> FunctionCall {
    unnest_call([(&columns[0], external_placeholder(2)), (&columns[1], external_placeholder(3))])
}

#[async_trait]
impl SnapshotStore for SnapshotRepository {
    async fn create(&self, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
        let sql = snapshot_create_sql()?;
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(params.id.as_str())
            .bind(params.snapshot_queries.map(serde_json::to_value).transpose()?)
            .bind(params.snapshot_temporal_queries.map(serde_json::to_value).transpose()?)
            .bind(params.snapshot_lifetime.map(ToString::to_string))
            .bind(params.snapshot_priority)
            .bind(params.endpoint.map(Url::as_str))
            .bind(params.receiver_info_json)
            .bind(<&str>::from(params.status))
            .bind(params.queries_details_json)
            .bind(params.temporal_queries_details_json)
            .bind(params.created_at)
            .bind(params.modified_at)
            .bind(params.expires_at)
            .bind(params.last_used_at)
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(())
    }

    async fn get(&self, id: &Uri) -> Result<Option<Snapshot>, SnapshotError> {
        let mut select = snapshot_select();
        select.and_where(Expr::col(Snapshots::Id).eq(id.as_str()));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(self.pool())
            .await
            .map_err(persistence_error)?;
        match row {
            Some(row) => Ok(Some(row_to_snapshot(&row).map_err(persistence_error)?)),
            None => Ok(None),
        }
    }

    async fn exists(&self, id: &Uri) -> Result<bool, SnapshotError> {
        let statement = Query::select()
            .expr(Expr::val(1))
            .from(Snapshots::Table)
            .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(row.is_some())
    }

    async fn next_preparing(&self) -> Result<Option<Snapshot>, SnapshotError> {
        let mut select = snapshot_select();
        select
            .and_where(Expr::col(Snapshots::Status).eq("preparing"))
            .order_by(Snapshots::SnapshotPriority, Order::Desc)
            .order_by(Snapshots::CreatedAt, Order::Asc)
            .limit(1);
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(self.pool())
            .await
            .map_err(persistence_error)?;
        row.as_ref().map(row_to_snapshot).transpose().map_err(persistence_error)
    }

    async fn update_lifetime_priority(
        &self,
        id: &Uri,
        lifetime: Option<&Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> Result<u64, SnapshotError> {
        let statement = Query::update()
            .table(Snapshots::Table)
            .value(
                Snapshots::SnapshotLifetime,
                Func::coalesce([Expr::val(lifetime.map(ToString::to_string)), Expr::col(Snapshots::SnapshotLifetime)]),
            )
            .value(
                Snapshots::SnapshotPriority,
                Func::coalesce([Expr::val(priority.map(|p| i16::from(p.as_u8()))), Expr::col(Snapshots::SnapshotPriority)]),
            )
            .value(
                Snapshots::ExpiresAt,
                Func::coalesce([Expr::val(new_expires_at), Expr::col(Snapshots::ExpiresAt)]),
            )
            .value(Snapshots::ModifiedAt, now())
            .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(result.rows_affected())
    }

    async fn update_status(
        &self,
        id: &Uri,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<u64, SnapshotError> {
        let statement = Query::update()
            .table(Snapshots::Table)
            .value(Snapshots::Status, cast(Expr::val(<&str>::from(status)), PostgresType::SnapshotStatus))
            .value(Snapshots::QueriesDetails, Expr::val(queries_details.and_then(|d| serde_json::to_value(d).ok())))
            .value(
                Snapshots::TemporalQueriesDetails,
                Expr::val(temporal_queries_details.and_then(|d| serde_json::to_value(d).ok())),
            )
            .value(Snapshots::ModifiedAt, now())
            .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
            .and_where(Expr::col(Snapshots::Status).eq("preparing"))
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(result.rows_affected())
    }

    async fn delete(&self, id: &Uri) -> Result<u64, SnapshotError> {
        delete_snapshot(self.pool(), id).await
    }

    async fn match_by_filter(&self, filter: &SnapshotPurgeFilter) -> Result<Vec<Uri>, SnapshotError> {
        let (sql, values) = build_match_query(filter).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(self.pool())
            .await
            .map_err(persistence_error)?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let raw: String = row.try_get("id").map_err(persistence_error)?;
            let parsed = raw
                .parse::<Uri>()
                .map_err(|e| SnapshotError::internal(format!("Invalid snapshot id '{raw}' in snapshots table: {e}")))?;
            ids.push(parsed);
        }
        Ok(ids)
    }

    async fn delete_one(&self, id: &Uri) -> Result<u64, SnapshotError> {
        delete_snapshot(self.pool(), id).await
    }

    async fn clone_snapshot(&self, source_id: &Uri, new_id: &Uri, new_expires_at: DateTime<Utc>) -> Result<Snapshot, SnapshotError> {
        let mut tx = self.pool().begin().await.map_err(persistence_error)?;

        let control_sql = clone_snapshot_sql()?;
        sqlx::query(sqlx::AssertSqlSafe(control_sql.as_str()))
            .bind(source_id.as_str())
            .bind(new_id.as_str())
            .bind(new_expires_at)
            .execute(&mut *tx)
            .await
            .map_err(persistence_error)?;

        let entities_sql = clone_rows_sql(SnapshotEntities::Table, SnapshotEntities::EntityId, SnapshotEntities::EntityData)?;
        sqlx::query(sqlx::AssertSqlSafe(entities_sql.as_str()))
            .bind(source_id.as_str())
            .bind(new_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(persistence_error)?;

        let temporal_sql = clone_rows_sql(
            SnapshotTemporalEntities::Table,
            SnapshotTemporalEntities::EntityId,
            SnapshotTemporalEntities::TemporalData,
        )?;
        sqlx::query(sqlx::AssertSqlSafe(temporal_sql.as_str()))
            .bind(source_id.as_str())
            .bind(new_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(persistence_error)?;

        tx.commit().await.map_err(persistence_error)?;

        self.get(new_id).await?.ok_or_else(|| SnapshotError::not_found(new_id))
    }

    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
        let statement = Query::delete()
            .from_table(Snapshots::Table)
            .and_where(Expr::col(Snapshots::ExpiresAt).lte(cutoff))
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(result.rows_affected())
    }

    async fn touch_last_used(&self, id: &Uri) -> Result<(), SnapshotError> {
        let statement = Query::update()
            .table(Snapshots::Table)
            .value(Snapshots::LastUsedAt, now())
            .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(())
    }

    async fn insert_entities(&self, snapshot_id: &Uri, entities: &[SnapshotEntityInput<'_>]) -> Result<(), SnapshotError> {
        let sql = batch_insert_sql(SnapshotEntities::Table, SnapshotEntities::EntityData)?;
        run_batch_insert(self.pool(), &sql, snapshot_id, entities).await
    }

    async fn insert_temporal_entities(&self, snapshot_id: &Uri, entities: &[SnapshotEntityInput<'_>]) -> Result<(), SnapshotError> {
        let sql = batch_insert_sql(SnapshotTemporalEntities::Table, SnapshotTemporalEntities::TemporalData)?;
        run_batch_insert(self.pool(), &sql, snapshot_id, entities).await
    }

    async fn get_entity(&self, snapshot_id: &Uri, entity_id: &Uri) -> Result<Option<Value>, SnapshotError> {
        get_snapshot_row(self.pool(), Alias::new("snapshot_entities"), Alias::new("entity_data"), snapshot_id, entity_id).await
    }

    async fn query_entities(
        &self,
        snapshot_id: &Uri,
        entity_type: Option<&TypeExprRaw>,
        entity_ids: Option<&[Uri]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), SnapshotError> {
        query_snapshot_table(&SnapshotQueryParams {
            pool: self.pool(),
            table: "snapshot_entities",
            data_column: "entity_data",
            snapshot_id,
            entity_type,
            entity_ids,
            limit,
            offset,
        })
        .await
    }

    async fn get_temporal_entity(&self, snapshot_id: &Uri, entity_id: &Uri) -> Result<Option<Value>, SnapshotError> {
        get_snapshot_row(
            self.pool(),
            Alias::new("snapshot_temporal_entities"),
            Alias::new("temporal_data"),
            snapshot_id,
            entity_id,
        )
        .await
    }

    async fn query_temporal_entities(
        &self,
        snapshot_id: &Uri,
        entity_type: Option<&TypeExprRaw>,
        entity_ids: Option<&[Uri]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), SnapshotError> {
        query_snapshot_table(&SnapshotQueryParams {
            pool: self.pool(),
            table: "snapshot_temporal_entities",
            data_column: "temporal_data",
            snapshot_id,
            entity_type,
            entity_ids,
            limit,
            offset,
        })
        .await
    }

    async fn snapshot_distinct_types(&self, snapshot_id: &Uri) -> Result<Vec<EntityTypeName>, SnapshotError> {
        let statement = Query::select()
            .distinct()
            .expr_as(entity_type_text(SnapshotEntities::Table), TypeColumn)
            .from(SnapshotEntities::Table)
            .and_where(Expr::col(SnapshotEntities::SnapshotId).eq(snapshot_id.as_str()))
            .and_where(SimpleExpr::from(Expr::col(SnapshotEntities::EntityData)).jsonb_has_key(Expr::val("type")))
            .order_by(TypeColumn, Order::Asc)
            .to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(self.pool())
            .await
            .map_err(persistence_error)?;
        let mut types = Vec::with_capacity(rows.len());
        for row in &rows {
            if let Some(raw) = row.try_get::<Option<String>, _>("t").map_err(persistence_error)? {
                types.push(parse_snapshot_entity_type(raw)?);
            }
        }
        Ok(types)
    }

    async fn snapshot_type_info(&self, snapshot_id: &Uri, type_name: &EntityTypeName) -> Result<Option<SnapshotTypeInfo>, SnapshotError> {
        let count = Query::select()
            .expr_as(count_all(), CountColumn)
            .from(SnapshotEntities::Table)
            .and_where(Expr::col(SnapshotEntities::SnapshotId).eq(snapshot_id.as_str()))
            .and_where(Expr::expr(entity_type_text(SnapshotEntities::Table)).eq(type_name.as_str()))
            .to_owned();
        let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
        let count_row = sqlx::query_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
            .fetch_one(self.pool())
            .await
            .map_err(persistence_error)?;
        let count: i64 = count_row.try_get("cnt").map_err(persistence_error)?;
        if count == 0 {
            return Ok(None);
        }

        let mut attributes = distinct_attribute_keys();
        attributes
            .and_where(Expr::col(SnapshotEntities::SnapshotId).eq(snapshot_id.as_str()))
            .and_where(Expr::expr(entity_type_text(SnapshotEntities::Table)).eq(type_name.as_str()));
        let (attr_sql, attr_values) = attributes.build_sqlx(PostgresQueryBuilder);
        let attr_rows = sqlx::query_with(sqlx::AssertSqlSafe(attr_sql.as_str()), attr_values)
            .fetch_all(self.pool())
            .await
            .map_err(persistence_error)?;
        let mut attrs = Vec::with_capacity(attr_rows.len());
        for row in &attr_rows {
            let raw: String = row.try_get("key").map_err(persistence_error)?;
            attrs.push(parse_snapshot_attribute_name(raw)?);
        }
        Ok(Some((count, attrs)))
    }

    async fn snapshot_distinct_attributes(&self, snapshot_id: &Uri) -> Result<Vec<AttributeName>, SnapshotError> {
        let mut attributes = distinct_attribute_keys();
        attributes.and_where(Expr::col(SnapshotEntities::SnapshotId).eq(snapshot_id.as_str()));
        let (sql, values) = attributes.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(self.pool())
            .await
            .map_err(persistence_error)?;
        let mut attrs = Vec::with_capacity(rows.len());
        for row in &rows {
            let raw: String = row.try_get("key").map_err(persistence_error)?;
            attrs.push(parse_snapshot_attribute_name(raw)?);
        }
        Ok(attrs)
    }

    async fn snapshot_attribute_info(&self, snapshot_id: &Uri, attr_name: &AttributeName) -> Result<Option<SnapshotAttributeInfo>, SnapshotError> {
        let count = Query::select()
            .expr_as(count_all(), CountColumn)
            .from(SnapshotEntities::Table)
            .and_where(Expr::col(SnapshotEntities::SnapshotId).eq(snapshot_id.as_str()))
            .and_where(SimpleExpr::from(Expr::col(SnapshotEntities::EntityData)).jsonb_has_key(Expr::val(attr_name.as_str())))
            .to_owned();
        let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
        let count_row = sqlx::query_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
            .fetch_one(self.pool())
            .await
            .map_err(persistence_error)?;
        let count: i64 = count_row.try_get("cnt").map_err(persistence_error)?;
        if count == 0 {
            return Ok(None);
        }

        let type_statement = Query::select()
            .distinct()
            .expr_as(entity_type_text(SnapshotEntities::Table), TypeColumn)
            .from(SnapshotEntities::Table)
            .and_where(Expr::col(SnapshotEntities::SnapshotId).eq(snapshot_id.as_str()))
            .and_where(SimpleExpr::from(Expr::col(SnapshotEntities::EntityData)).jsonb_has_key(Expr::val(attr_name.as_str())))
            .and_where(SimpleExpr::from(Expr::col(SnapshotEntities::EntityData)).jsonb_has_key(Expr::val("type")))
            .order_by(TypeColumn, Order::Asc)
            .to_owned();
        let (type_sql, type_values) = type_statement.build_sqlx(PostgresQueryBuilder);
        let type_rows = sqlx::query_with(sqlx::AssertSqlSafe(type_sql.as_str()), type_values)
            .fetch_all(self.pool())
            .await
            .map_err(persistence_error)?;
        let mut types = Vec::with_capacity(type_rows.len());
        for row in &type_rows {
            if let Some(raw) = row.try_get::<Option<String>, _>("t").map_err(persistence_error)? {
                types.push(parse_snapshot_entity_type(raw)?);
            }
        }
        Ok(Some((count, types)))
    }
}

/// The `SELECT DISTINCT key FROM snapshot_entities, jsonb_object_keys(entity_data) AS key WHERE key NOT IN (…) ORDER BY key`
/// base a distinct-attribute read shares, before the caller adds its scope.
fn distinct_attribute_keys() -> SelectStatement {
    Query::select()
        .distinct()
        .column(KeyColumn)
        .from(SnapshotEntities::Table)
        .from_function(jsonb_object_keys(Expr::col((SnapshotEntities::Table, SnapshotEntities::EntityData))), KeyColumn)
        .and_where(Expr::col(KeyColumn).is_not_in(NON_ATTRIBUTE_KEYS))
        .order_by(KeyColumn, Order::Asc)
        .to_owned()
}

/// Delete one snapshot by identifier.
async fn delete_snapshot(pool: &PgPool, id: &Uri) -> Result<u64, SnapshotError> {
    let statement = Query::delete()
        .from_table(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(id.as_str()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

/// Query one snapshot backing table with the common selector and pagination contract.
async fn query_snapshot_table(params: &SnapshotQueryParams<'_>) -> Result<(Vec<Value>, i64), SnapshotError> {
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

/// Parse one entity type decoded from a snapshot projection.
fn parse_snapshot_entity_type(raw: String) -> Result<EntityTypeName, SnapshotError> {
    raw.parse::<EntityTypeName>()
        .map_err(|error| SnapshotError::internal(format!("invalid snapshot entity type '{raw}' in database: {error}")))
}

/// Parse one attribute name decoded from a snapshot projection.
fn parse_snapshot_attribute_name(raw: String) -> Result<AttributeName, SnapshotError> {
    raw.parse::<AttributeName>()
        .map_err(|error| SnapshotError::internal(format!("invalid snapshot attribute name '{raw}' in database: {error}")))
}

/// The bulk snapshot-row upsert for one backing table, its entity ids and
/// documents bound by the caller's chain as `$2`/`$3`.
fn batch_insert_sql(table: impl IntoIden + Copy + 'static, data_column: impl IntoIden + Copy + 'static) -> Result<String, SnapshotError> {
    let columns = [
        UnnestColumn::new(SnapshotEntities::EntityId, PostgresType::TextArray),
        UnnestColumn::new(data_column, PostgresType::JsonbArray),
    ];
    let call = snapshot_rows_unnest(&columns);

    let mut select = Query::select();
    select
        .expr(external_placeholder(1))
        .expr(Expr::col((D, SnapshotEntities::EntityId)))
        .expr(Expr::col((D, data_column.into_iden())))
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(table).columns([
        SnapshotEntities::SnapshotId.into_iden(),
        SnapshotEntities::EntityId.into_iden(),
        data_column.into_iden(),
    ]);
    insert.select_from(select).map_err(persistence_error)?.on_conflict(
        OnConflict::columns([SnapshotEntities::SnapshotId, SnapshotEntities::EntityId])
            .update_column(data_column)
            .to_owned(),
    );

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// The `UNNEST` staging alias, `d`, a batch of snapshot rows is expanded under.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// Bind and run one bulk snapshot-row insert.
async fn run_batch_insert(pool: &PgPool, sql: &str, snapshot_id: &Uri, entities: &[SnapshotEntityInput<'_>]) -> Result<(), SnapshotError> {
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

/// Retrieve one prepared snapshot row by snapshot and entity identifiers.
async fn get_snapshot_row(pool: &PgPool, table: Alias, data_column: Alias, snapshot_id: &Uri, entity_id: &Uri) -> Result<Option<Value>, SnapshotError> {
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

#[cfg(test)]
mod tests {
    use crate::snapshot::store::{batch_insert_sql, clone_snapshot_sql, distinct_attribute_keys, snapshot_create_sql};
    use andromeda_postgresql_schema::snapshot::{SnapshotEntities, SnapshotTemporalEntities};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_create_casts_its_status_marker_and_stamps_no_clock() {
        let sql = snapshot_create_sql().expect("the fixed snapshot insert builds");
        assert!(sql.contains("$8::snapshot_status"), "{sql}");
        for marker in 1..=14 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$15"), "the create binds exactly fourteen values: {sql}");
    }

    #[test]
    fn the_clone_copies_the_control_record_and_stamps_fresh_clocks() {
        let sql = clone_snapshot_sql().expect("the fixed clone builds");
        assert!(sql.starts_with(r#"INSERT INTO "snapshots""#), "{sql}");
        assert!(sql.contains("SELECT $2,"), "the new id is the copied identifier: {sql}");
        assert!(sql.contains("NOW(), NOW()"), "created_at and modified_at are stamped fresh: {sql}");
        assert!(sql.contains(r#"WHERE "id" = ($1)"#), "{sql}");
    }

    #[test]
    fn a_batch_insert_upserts_the_document_on_the_snapshot_and_entity_grain() {
        let entities = batch_insert_sql(SnapshotEntities::Table, SnapshotEntities::EntityData).expect("builds");
        assert!(
            entities.contains(r#"ON CONFLICT ("snapshot_id", "entity_id") DO UPDATE SET "entity_data" = "excluded"."entity_data""#),
            "{entities}"
        );
        assert!(entities.contains("AS d(entity_id, entity_data)"), "{entities}");
        let temporal = batch_insert_sql(SnapshotTemporalEntities::Table, SnapshotTemporalEntities::TemporalData).expect("builds");
        assert!(temporal.contains(r#"INSERT INTO "snapshot_temporal_entities""#), "{temporal}");
        assert!(temporal.contains(r#""temporal_data" = "excluded"."temporal_data""#), "{temporal}");
    }

    #[test]
    fn distinct_attributes_skip_the_keyword_members_and_walk_the_object_keys() {
        let sql = distinct_attribute_keys().to_string(PostgresQueryBuilder);
        assert!(sql.contains("jsonb_object_keys"), "{sql}");
        assert!(sql.contains(r#""key" NOT IN ('id', 'type', '@context')"#), "{sql}");
        assert!(sql.contains(r#"SELECT DISTINCT "key""#), "{sql}");
    }
}
