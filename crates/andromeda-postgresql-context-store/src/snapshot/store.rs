//! `SnapshotStore` trait implementation for `SnapshotRepository`.
//!
//! The implementation dispatches to the statement modules by concern: the
//! control record ([`crate::snapshot::lifecycle`]), the captured rows
//! ([`crate::snapshot::content`]) and the summaries over them
//! ([`crate::snapshot::introspection`]).

use crate::snapshot::{
    content::{batch_insert_sql, clone_rows_sql, get_snapshot_row, query_snapshot_table, run_batch_insert, run_clone_rows},
    introspection::{snapshot_attribute_info, snapshot_distinct_attributes, snapshot_distinct_types, snapshot_type_info},
    lifecycle::{
        clone_snapshot_sql,
        create_snapshot,
        delete_expired_snapshots,
        delete_snapshot,
        match_snapshots_by_filter,
        next_preparing_snapshot,
        read_snapshot,
        snapshot_exists,
        touch_last_used,
        update_lifetime_priority,
        update_status,
    },
    params::SnapshotQueryParams,
    persistence_error,
    repo::SnapshotRepository,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_postgresql_schema::snapshot::{SnapshotEntities, SnapshotTemporalEntities};
use andromeda_snapshot::{
    error::SnapshotError,
    store::{InsertSnapshotParams, SnapshotAttributeInfo, SnapshotEntityInput, SnapshotStore, SnapshotTypeInfo},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::Alias;
use serde_json::Value;

#[async_trait]
impl SnapshotStore for SnapshotRepository {
    async fn create(&self, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
        create_snapshot(self.pool(), params).await
    }

    async fn get(&self, id: &Uri) -> Result<Option<Snapshot>, SnapshotError> {
        read_snapshot(self.pool(), id).await
    }

    async fn exists(&self, id: &Uri) -> Result<bool, SnapshotError> {
        snapshot_exists(self.pool(), id).await
    }

    async fn next_preparing(&self) -> Result<Option<Snapshot>, SnapshotError> {
        next_preparing_snapshot(self.pool()).await
    }

    async fn update_lifetime_priority(
        &self,
        id: &Uri,
        lifetime: Option<&Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> Result<u64, SnapshotError> {
        update_lifetime_priority(self.pool(), id, lifetime, priority, new_expires_at).await
    }

    async fn update_status(
        &self,
        id: &Uri,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<u64, SnapshotError> {
        update_status(self.pool(), id, status, queries_details, temporal_queries_details).await
    }

    async fn delete(&self, id: &Uri) -> Result<u64, SnapshotError> {
        delete_snapshot(self.pool(), id).await
    }

    async fn match_by_filter(&self, filter: &SnapshotPurgeFilter) -> Result<Vec<Uri>, SnapshotError> {
        match_snapshots_by_filter(self.pool(), filter).await
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
        run_clone_rows(&mut tx, &entities_sql, source_id, new_id).await?;

        let temporal_sql = clone_rows_sql(
            SnapshotTemporalEntities::Table,
            SnapshotTemporalEntities::EntityId,
            SnapshotTemporalEntities::TemporalData,
        )?;
        run_clone_rows(&mut tx, &temporal_sql, source_id, new_id).await?;

        tx.commit().await.map_err(persistence_error)?;

        self.get(new_id).await?.ok_or_else(|| SnapshotError::not_found(new_id))
    }

    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
        delete_expired_snapshots(self.pool(), cutoff).await
    }

    async fn touch_last_used(&self, id: &Uri) -> Result<(), SnapshotError> {
        touch_last_used(self.pool(), id).await
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
        snapshot_distinct_types(self.pool(), snapshot_id).await
    }

    async fn snapshot_type_info(&self, snapshot_id: &Uri, type_name: &EntityTypeName) -> Result<Option<SnapshotTypeInfo>, SnapshotError> {
        snapshot_type_info(self.pool(), snapshot_id, type_name).await
    }

    async fn snapshot_distinct_attributes(&self, snapshot_id: &Uri) -> Result<Vec<AttributeName>, SnapshotError> {
        snapshot_distinct_attributes(self.pool(), snapshot_id).await
    }

    async fn snapshot_attribute_info(&self, snapshot_id: &Uri, attr_name: &AttributeName) -> Result<Option<SnapshotAttributeInfo>, SnapshotError> {
        snapshot_attribute_info(self.pool(), snapshot_id, attr_name).await
    }
}
