//! [`TemporalHistoryReader`] implementation for [`TemporalRepository`].
//!
//! Bridges the consumer-declared temporal read port to the PostgreSQL-backed
//! store. Stage 1 (entity ID resolution) includes the `q`-attribute
//! dictionary lookup and `attribute_current` barrier wait that the service
//! used to own; stage 2 (per-entity history) runs the SQL builder and decodes
//! each `PgRow` into a [`TemporalHistoryRow`].

use crate::{
    repository::temporal::{
        history_row_decode::{decode_aggregation_row, decode_history_row},
        repo::TemporalRepository,
    },
    service::temporal::store::{
        AggregationPushdown,
        EntityIdOutcome,
        HistoryVisibility,
        TemporalAggregationRow,
        TemporalHistoryOutcome,
        TemporalHistoryReader,
        TemporalStorageProfile,
        TemporalStoreError,
    },
};
use andromeda_ngsi_ld_query::{identifier::parse_id, pagination::CountPolicy, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_query::q_predicate::projection::referenced_attribute_names;
use andromeda_postgresql_temporal_query::{history::build_temporal_history_query, stage1::build_stage1_entity_ids};
use async_trait::async_trait;
use sqlx::Row;

#[async_trait]
impl TemporalHistoryReader for TemporalRepository {
    fn profile(&self) -> TemporalStorageProfile {
        TemporalStorageProfile {
            aggregation_pushdown: if self.timescale_features.has_continuous_aggregates {
                AggregationPushdown::Materialised
            } else {
                AggregationPushdown::OnTheFly
            },
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn query_entity_ids(&self, compiled: &CompiledTemporalQuery) -> Result<EntityIdOutcome, TemporalStoreError> {
        let attribute_names = if let Some(q_expr) = &compiled.entities.q_expr {
            let mut names = Vec::new();
            referenced_attribute_names(q_expr, &mut names);
            let resolved = self
                .attribute_names
                .lookup(&names)
                .await
                .map_err(|e| TemporalStoreError::Query(e.to_string()))?;
            if !resolved.is_empty() {
                if let Some(barrier) = &self.attribute_current {
                    barrier.wait_for_pending_writes().await;
                }
            }
            resolved
        } else {
            Default::default()
        };

        let stage1 = build_stage1_entity_ids(compiled, &attribute_names).map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let entity_id_rows = self
            .execute_temporal_query(&stage1.sql, &stage1.bind_values)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let entity_ids: Vec<String> = entity_id_rows.iter().filter_map(|r| r.try_get("entity_id").ok()).collect();

        let total_count = if compiled.pagination.count == CountPolicy::Emit {
            let count_sql = stage1
                .count_sql
                .as_ref()
                .ok_or_else(|| TemporalStoreError::Query("Count SQL not built".to_string()))?;
            let count_rows = self
                .execute_temporal_query(count_sql, &stage1.bind_values)
                .await
                .map_err(|e| TemporalStoreError::Query(e.to_string()))?;
            count_rows.first().and_then(|r| r.try_get::<i64, _>("count").ok())
        } else {
            None
        };

        Ok(EntityIdOutcome { entity_ids, total_count })
    }

    async fn query_entity_history(&self, entity_id: &str, compiled: &CompiledTemporalQuery) -> Result<TemporalHistoryOutcome, TemporalStoreError> {
        let uri = parse_id(entity_id).map_err(|e| TemporalStoreError::Query(e.to_string()))?;
        let stage2 = build_temporal_history_query(&uri, compiled).map_err(|e| TemporalStoreError::Query(e.to_string()))?;
        let pg_rows = self
            .execute_temporal_query(&stage2.sql, &stage2.bind_values)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let mut rows = Vec::with_capacity(pg_rows.len());
        for pg_row in &pg_rows {
            rows.push(decode_history_row(pg_row)?);
        }

        Ok(TemporalHistoryOutcome { rows })
    }

    async fn query_entity_aggregation(&self, entity_id: &str, compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalAggregationRow>, TemporalStoreError> {
        let uri = parse_id(entity_id).map_err(|e| TemporalStoreError::Query(e.to_string()))?;
        let stage2 = build_temporal_history_query(&uri, compiled).map_err(|e| TemporalStoreError::Query(e.to_string()))?;
        let pg_rows = self
            .execute_temporal_query(&stage2.sql, &stage2.bind_values)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let mut rows = Vec::with_capacity(pg_rows.len());
        for pg_row in &pg_rows {
            rows.push(decode_aggregation_row(pg_row)?);
        }

        Ok(rows)
    }
}
