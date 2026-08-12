//! [`TemporalHistoryReader`] implementation for [`TemporalRepository`].
//!
//! Bridges the consumer-declared temporal read port to the PostgreSQL-backed
//! store. Stage 1 (entity ID resolution) includes the `q`-attribute
//! dictionary lookup that the service used to own; stage 2 (per-entity
//! history) runs the SQL builder and decodes each `PgRow` into a
//! [`TemporalHistoryRow`].

use crate::{
    error::TemporalStoreError,
    history_row_decode::{decode_aggregation_row, decode_history_row, decode_unit_code_id},
    repo::TemporalRepository,
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;
use andromeda_ngsi_ld_query::{pagination::CountPolicy, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_query::projected_attribute_names::projected_attribute_names;
use andromeda_postgresql_temporal_query::{
    aggregation::{AggregationTarget, ensure_single_unit_per_target},
    history::build_temporal_history_query,
    stage1::build_stage1_entity_ids,
};
use andromeda_temporal::{
    error::TemporalError,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    port::history_reader::{AggregationPushdown, HistoryVisibility, TemporalHistoryOutcome, TemporalHistoryReader, TemporalReaderCapabilities},
};
use async_trait::async_trait;
use sqlx::Row;
use std::collections::HashMap;

#[async_trait]
impl TemporalHistoryReader for TemporalRepository {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            // Aggregation runs over the raw hypertable. A continuous aggregate
            // has to bucket on the partition column, which is `recorded_at`, and
            // clause 5.2.21 admits no such Temporal Property, so a materialised
            // aggregate would answer a question the API cannot ask.
            aggregation: AggregationPushdown::OnTheFly,
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalError> {
        self.temporal_entity_exists(entity_id).await.map_err(TemporalError::from)
    }

    async fn entity_types(&self, entity_ids: &[Uri]) -> Result<HashMap<Uri, Vec<EntityTypeName>>, TemporalError> {
        self.temporal_entity_types(entity_ids).await.map_err(TemporalError::from)
    }

    async fn query_entity_ids(&self, compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
        // A temporal query orders by time rather than by distance, so it
        // contributes no ordering terms.
        let names = projected_attribute_names(compiled.entities.q_expr.as_ref(), compiled.entities.geo.as_ref(), &[]);
        // The projection is written inside the transaction that writes the
        // entity, so a dictionary hit is already backed by committed projection
        // rows and there is nothing to wait for.
        let attribute_names = if names.is_empty() {
            Default::default()
        } else {
            self.attribute_names
                .lookup(&names)
                .await
                .map_err(|e| TemporalStoreError::Query(e.to_string()))?
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

        // The aggregation SQL groups by unit, so one target yields one row per
        // unit its instances were recorded in. A unit-sensitive aggregate over
        // more than one of them has no meaning and the broker never converts,
        // so the whole request is refused rather than answered per unit.
        let mut rows = Vec::with_capacity(pg_rows.len());
        let mut groups = Vec::with_capacity(pg_rows.len());
        for pg_row in &pg_rows {
            let row = decode_aggregation_row(pg_row).map_err(TemporalError::from)?;
            let unit_code_id = decode_unit_code_id(pg_row).map_err(TemporalError::from)?;
            groups.push((
                AggregationTarget {
                    // clone: the target outlives the decoded row only as a group key
                    attribute_name: row.attribute_name.clone(),
                    dataset_id: row.dataset_id.clone(),
                    bucket_start: row.bucket_start,
                },
                unit_code_id,
            ));
            rows.push(row);
        }
        ensure_single_unit_per_target(
            groups.iter().map(|(target, unit_code_id)| (target, *unit_code_id)),
            &compiled.representation.aggr_methods,
        )
        .map_err(TemporalStoreError::from)
        .map_err(TemporalError::from)?;

        Ok(rows)
    }
}
