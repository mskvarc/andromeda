//! [`TemporalHistoryReader`] implementation for [`TemporalRepository`].
//!
//! Bridges the consumer-declared temporal read port to the PostgreSQL-backed
//! store. Stage 1 (entity ID resolution) includes the `q`-attribute
//! dictionary lookup that the service used to own; stage 2 (per-entity
//! history) runs the SQL builder and decodes each `PgRow` into a
//! [`TemporalHistoryRow`].

use crate::{
    aggregation_row_decode::{decode_aggregation_group_span, decode_aggregation_row},
    error::TemporalStoreError,
    history_row_decode::decode_history_row,
    repo::TemporalRepository,
    temporal_query_execution::TemporalStatement,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;
use andromeda_ngsi_ld_query::{pagination::CountPolicy, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_query::{
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    projected_attribute_names::projected_attribute_names,
    projected_type_names::projected_type_names,
};
use andromeda_postgresql_temporal_query::{
    aggregation_group::ensure_uniform_aggregation_groups,
    history::build_temporal_history_query,
    stage1::build_stage1_entity_ids,
};
use andromeda_temporal::{
    error::TemporalError,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    port::history_reader::{
        AggregationPushdown,
        HistoryVisibility,
        RecordedValueKind,
        TemporalHistoryOutcome,
        TemporalHistoryReader,
        TemporalReaderCapabilities,
    },
};
use async_trait::async_trait;
use sqlx::Row;
use std::collections::{HashMap, HashSet};

/// The neutral value shape a stored value-kind code stands for; a tombstone
/// carries no value and so no shape.
const fn recorded_value_kind(kind: ValueKind) -> Option<RecordedValueKind> {
    match kind {
        ValueKind::Tombstone => None,
        ValueKind::Number => Some(RecordedValueKind::Number),
        ValueKind::String => Some(RecordedValueKind::String),
        ValueKind::Boolean => Some(RecordedValueKind::Boolean),
        ValueKind::Geometry => Some(RecordedValueKind::Geometry),
        ValueKind::Object => Some(RecordedValueKind::Object),
        ValueKind::DateTime => Some(RecordedValueKind::DateTime),
        ValueKind::Uri => Some(RecordedValueKind::Uri),
    }
}

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

    async fn recorded_value_kinds(&self, attributes: &[AttributeName]) -> Result<HashMap<AttributeName, HashSet<RecordedValueKind>>, TemporalError> {
        let recorded = self.recorded_value_kinds(attributes).await.map_err(TemporalError::from)?;
        Ok(recorded
            .into_iter()
            .map(|(name, kinds)| (name, kinds.into_iter().filter_map(recorded_value_kind).collect()))
            .collect())
    }

    async fn query_entity_ids(&self, compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
        // A temporal query orders by time rather than by distance, so it
        // contributes no ordering terms.
        let attribute_names = projected_attribute_names(&[], compiled.entities.q_expr.as_ref(), compiled.entities.geo.as_ref(), &[]);
        // The projection is written inside the transaction that writes the
        // entity, so a dictionary hit is already backed by committed projection
        // rows and there is nothing to wait for.
        let attributes = if attribute_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.attribute_names
                .lookup(&attribute_names)
                .await
                .map_err(|e| TemporalStoreError::Query(e.to_string()))?
        };
        // The catalogue carries type keys, so a type selection resolves through
        // the same dictionary the write path minted them in.
        let type_names = projected_type_names(&compiled.entities);
        let types = if type_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.entity_type_names
                .lookup(&type_names)
                .await
                .map_err(|e| TemporalStoreError::Query(e.to_string()))?
        };
        let names = QueryNames { attributes, types };

        let stage1 = build_stage1_entity_ids(compiled, &names).map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let entity_id_rows = self
            .execute_temporal_query(TemporalStatement::Stage1, stage1.select)
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
            let count = stage1.count.ok_or_else(|| TemporalStoreError::Query("Count SQL not built".to_string()))?;
            let count_rows = self
                .execute_temporal_query(TemporalStatement::Stage1Count, count)
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
        let stage2 = build_temporal_history_query(entity_id, compiled);
        let pg_rows = self
            .execute_temporal_query(TemporalStatement::History, stage2)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let mut rows = Vec::with_capacity(pg_rows.len());
        for pg_row in &pg_rows {
            rows.push(decode_history_row(pg_row).map_err(TemporalError::from)?);
        }

        Ok(TemporalHistoryOutcome { observations: rows })
    }

    async fn query_entity_aggregation(&self, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        let stage2 = build_temporal_history_query(entity_id, compiled);
        let pg_rows = self
            .execute_temporal_query(TemporalStatement::Aggregation, stage2)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        // Clause 4.5.19 gives one aggregate value per attribute, `datasetId` and
        // period, so the statement groups on exactly those and each row is one
        // rendered period. What the rows underneath a group disagreed about is
        // read first: a unit-sensitive aggregate over values that do not share a
        // unit, a family or a shape has no meaning, and the broker converts
        // nothing, so the request is refused before any aggregate of it is
        // decoded.
        let mut spans = Vec::with_capacity(pg_rows.len());
        for pg_row in &pg_rows {
            spans.push(decode_aggregation_group_span(pg_row).map_err(TemporalError::from)?);
        }
        ensure_uniform_aggregation_groups(&spans, &compiled.representation.aggr_methods)
            .map_err(TemporalStoreError::from)
            .map_err(TemporalError::from)?;

        let mut rows = Vec::with_capacity(pg_rows.len());
        for (pg_row, span) in pg_rows.iter().zip(&spans) {
            rows.push(decode_aggregation_row(pg_row, span).map_err(TemporalError::from)?);
        }

        Ok(rows)
    }
}
