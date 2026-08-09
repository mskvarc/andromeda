//! [`TemporalHistoryReader`] implementation for [`TemporalRepository`].
//!
//! Bridges the consumer-declared temporal read port to the PostgreSQL-backed
//! store. Stage 1 (entity ID resolution) includes the `q`-attribute
//! dictionary lookup and `attribute_current` barrier wait that the service
//! used to own; stage 2 (per-entity history) runs the SQL builder and decodes
//! each `PgRow` into a [`TemporalHistoryRow`].

use crate::{
    error::TemporalStoreError,
    history_row_decode::{decode_aggregation_row, decode_history_row},
    repo::TemporalRepository,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;
use andromeda_ngsi_ld_query::{pagination::CountPolicy, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_query::q_predicate::projection::referenced_attribute_names;
use andromeda_postgresql_temporal_query::{history::build_temporal_history_query, stage1::build_stage1_entity_ids};
use andromeda_temporal::{
    error::TemporalError,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    port::history_reader::{AggregationPushdown, HistoryVisibility, TemporalHistoryOutcome, TemporalHistoryReader, TemporalReaderCapabilities},
};
use async_trait::async_trait;
use sqlx::Row;

#[async_trait]
impl TemporalHistoryReader for TemporalRepository {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            aggregation: if self.timescale_features.has_continuous_aggregates {
                AggregationPushdown::Materialized
            } else {
                AggregationPushdown::OnTheFly
            },
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalError> {
        self.tea_exists_for_entity(entity_id).await.map_err(TemporalError::from)
    }

    async fn query_entity_ids(&self, compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
        let attribute_names = if let Some(q_expr) = &compiled.entities.q_expr {
            let mut names = Vec::new();
            referenced_attribute_names(q_expr, &mut names);
            let resolved = self
                .attribute_names
                .lookup(&names)
                .await
                .map_err(|e| TemporalStoreError::Query(e.to_string()))?;
            if !resolved.is_empty()
                && let Some(barrier) = &self.attribute_current
            {
                barrier.wait_for_pending_writes().await;
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

        let entity_ids = entity_id_rows
            .into_iter()
            .map(|row| {
                let raw: String = row.try_get("entity_id")?;
                raw.parse::<Uri>()
                    .map_err(|error| TemporalStoreError::Decode(format!("Invalid temporal entity_id '{raw}': {error}")))
            })
            .collect::<Result<Vec<_>, _>>()?;

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

        Ok(TemporalEntityIdOutcome {
            candidates: entity_ids
                .into_iter()
                .map(|entity_id| TemporalEntityCandidate {
                    entity_id,
                    selection_value: None,
                })
                .collect(),
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Applied { total: total_count },
        })
    }

    async fn query_entity_history(&self, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<TemporalHistoryOutcome, TemporalError> {
        let stage2 = build_temporal_history_query(entity_id, compiled).map_err(|e| TemporalStoreError::Query(e.to_string()))?;
        let pg_rows = self
            .execute_temporal_query(&stage2.sql, &stage2.bind_values)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let mut rows = Vec::with_capacity(pg_rows.len());
        for pg_row in &pg_rows {
            rows.push(decode_history_row(pg_row).map_err(TemporalError::from)?);
        }

        Ok(TemporalHistoryOutcome { observations: rows })
    }

    async fn query_entity_aggregation(&self, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        let stage2 = build_temporal_history_query(entity_id, compiled).map_err(|e| TemporalStoreError::Query(e.to_string()))?;
        let pg_rows = self
            .execute_temporal_query(&stage2.sql, &stage2.bind_values)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let mut rows = Vec::with_capacity(pg_rows.len());
        for pg_row in &pg_rows {
            rows.push(decode_aggregation_row(pg_row).map_err(TemporalError::from)?);
        }

        Ok(rows)
    }
}
