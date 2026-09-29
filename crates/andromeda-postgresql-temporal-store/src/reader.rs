//! [`TemporalHistoryReader`] implementation for [`TemporalRepository`].
//!
//! Bridges the consumer-declared temporal read port to the PostgreSQL-backed
//! store. Stage 1 (entity ID resolution) includes the `q`-attribute
//! dictionary lookup that the service used to own; stage 2 (per-entity
//! history) runs the SQL builder and decodes each `PgRow` into a
//! [`TemporalHistoryRow`].

use crate::{
    aggregation_group_key_decode::decode_group_keys,
    aggregation_row_decode::{decode_aggregation_group_span, decode_aggregation_row},
    error::TemporalStoreError,
    history_row_decode::decode_history_row,
    repo::TemporalRepository,
    scope_history_read::decode_scope_history,
    stored_attribute_shape::StoredAttributeShape,
    temporal_query_execution::TemporalStatement,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;
use andromeda_ngsi_ld_query::{cursor::PageCursor, pagination::CountPolicy, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_codes::attribute_kind_code::attribute_type_of;
use andromeda_postgresql_query::{
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    projected_attribute_names::projected_attribute_names,
    projected_type_names::projected_type_names,
};
use andromeda_postgresql_temporal_query::{
    aggregation_group::{PromotedGroupFacets, ensure_uniform_aggregation_groups},
    aggregation_grouping::promoted_facets,
    history::build_temporal_history_query,
    recorded_shape::RecordedShape,
    scope_history::{build_scope_history_query, scope_is_selected},
    stage1::build_stage1_entity_ids,
};
use andromeda_temporal::{
    error::TemporalError,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    port::history_reader::{
        AggregationPushdown,
        HistoryVisibility,
        RecordedAttributeShape,
        RecordedValueKind,
        TemporalHistoryObservation,
        TemporalHistoryOutcome,
        TemporalHistoryReader,
        TemporalReaderCapabilities,
    },
};
use async_trait::async_trait;
use sqlx::Row;
use std::collections::{HashMap, HashSet};

/// The neutral value shape one projected [`RecordedShape`] stands for.
///
/// The two vocabularies are the same set of shapes named on either side of the
/// port, one member per column of Tables 4.5.19.1-1, 4.5.19.1-2 and
/// 4.5.19.1-3, so the mapping is total: no shape the statement projects is
/// dropped on its way to the eligibility check, which is what would silently
/// widen or narrow a table column.
const fn recorded_value_kind(shape: RecordedShape) -> RecordedValueKind {
    match shape {
        RecordedShape::Number => RecordedValueKind::Number,
        RecordedShape::String => RecordedValueKind::String,
        RecordedShape::Boolean => RecordedValueKind::Boolean,
        RecordedShape::Geometry => RecordedValueKind::Geometry,
        RecordedShape::Object => RecordedValueKind::Object,
        RecordedShape::Array => RecordedValueKind::Array,
        RecordedShape::DateTime => RecordedValueKind::DateTime,
        RecordedShape::Date => RecordedValueKind::Date,
        RecordedShape::Time => RecordedValueKind::Time,
        RecordedShape::Uri => RecordedValueKind::Uri,
    }
}

/// The neutral shape one stored family-and-value-shape pair stands for.
///
/// Both halves cross the port, because clause 4.5.19.1's tables are not indexed
/// the same way: the datatype decides for a Property (Tables 4.5.19.1-1 and
/// 4.5.19.1-2) and the family alone decides for a Relationship (Table
/// 4.5.19.1-3).
const fn neutral_attribute_shape(stored: StoredAttributeShape) -> RecordedAttributeShape {
    RecordedAttributeShape {
        family: attribute_type_of(stored.kind),
        value_kind: recorded_value_kind(stored.shape),
    }
}

impl TemporalRepository {
    /// Read the reified temporal scope of one entity over the compiled window.
    ///
    /// Clause 4.18 makes the Scope "a special scope Property that is
    /// non-reified in the normalized Entity representation and reified in the
    /// temporal representation of an Entity", and Table 6.18.3.2-1 lets a
    /// request narrow to it through `attrs` — which annex C clause C.5.16 does
    /// with `attrs=speed,scope`. An `attrs` that names other members alone
    /// therefore skips the read entirely rather than reading rows the response
    /// would drop.
    async fn read_scope_history(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalHistoryObservation>, TemporalError> {
        if !scope_is_selected(compiled) {
            return Ok(Vec::new());
        }
        let statement = build_scope_history_query(schema_iden(target).as_ref(), entity_id, compiled);
        let pg_rows = self
            .execute_temporal_query(TemporalStatement::ScopeHistory, statement)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;
        decode_scope_history(&pg_rows).map_err(TemporalError::from)
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

    async fn entity_exists(&self, target: &ContextTarget, entity_id: &Uri) -> Result<bool, TemporalError> {
        self.temporal_entity_exists(target, entity_id).await.map_err(TemporalError::from)
    }

    async fn entity_types(&self, target: &ContextTarget, entity_ids: &[Uri]) -> Result<HashMap<Uri, Vec<EntityTypeName>>, TemporalError> {
        self.temporal_entity_types(target, entity_ids).await.map_err(TemporalError::from)
    }

    async fn recorded_attribute_shapes(
        &self,
        target: &ContextTarget,
        attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        let recorded = self.stored_attribute_shapes(target, attributes).await.map_err(TemporalError::from)?;
        Ok(recorded
            .into_iter()
            .map(|(name, shapes)| (name, shapes.into_iter().map(neutral_attribute_shape).collect()))
            .collect())
    }

    async fn query_entity_ids(&self, target: &ContextTarget, compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
        // Clause 5.7.4.3 confines a temporal ordering to the `id` member, which
        // is a column of the temporal catalogue and names no attribute, so the
        // dictionary lookup below has no ordering term to resolve.
        let attribute_names = projected_attribute_names([], compiled.entities.q_expr.as_ref(), compiled.entities.geo.as_ref(), &[]);
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

        // The statement compares identifiers under the collation the ordering
        // names, and PostgreSQL pre-imports only the `<locale>-x-icu` forms; a
        // keyword form clause 4.23.3 demonstrates has no object until one is
        // created, and ordering under it would fail the statement outright.
        if let Some(collation) = compiled.ordering.as_ref().and_then(|ordering| ordering.collation.as_ref()) {
            self.collations.ensure(collation).await.map_err(TemporalStoreError::from)?;
        }

        let stage1 = build_stage1_entity_ids(schema_iden(target).as_ref(), compiled, &names).map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let entity_id_rows = self
            .execute_temporal_query(TemporalStatement::Stage1, stage1.select)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        let mut entity_ids = entity_id_rows
            .into_iter()
            .map(|row| {
                let raw: String = row.try_get("entity_id")?;
                raw.parse::<Uri>()
                    .map_err(|error| TemporalStoreError::Decode(format!("Invalid temporal entity_id '{raw}': {error}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Clause 5.5.9.1: a backward or from-end keyset page was read descending
        // so one index scan could serve it; present it ascending, the order every
        // element page shows.
        if matches!(compiled.keyset, Some(PageCursor::Before(_) | PageCursor::End)) {
            entity_ids.reverse();
        }
        let entity_ids = entity_ids;

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

    async fn query_entity_history(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        let stage2 = build_temporal_history_query(schema_iden(target).as_ref(), entity_id, compiled);
        let pg_rows = self
            .execute_temporal_query(TemporalStatement::History, stage2)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;

        // Clause 4.18 reifies the Scope in the temporal representation, and it
        // is stored apart from attribute history because clause 4.5.4 gives it
        // no attribute stream, so the representation is assembled from two
        // reads. The scope leads, which is the order annex C clause C.5.16
        // renders it in.
        let mut observations = self.read_scope_history(target, entity_id, compiled).await?;
        observations.reserve(pg_rows.len());
        for pg_row in &pg_rows {
            observations.push(decode_history_row(pg_row).map_err(TemporalError::from)?);
        }

        Ok(TemporalHistoryOutcome { observations })
    }

    async fn query_entity_aggregation(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        let stage2 = build_temporal_history_query(schema_iden(target).as_ref(), entity_id, compiled);
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
        // A facet promoted to an `aggrGroupBy` key splits the groups rather than
        // contradicting them, so the guard skips its uniformity check (study
        // §4).
        let promoted = compiled
            .representation
            .grouping
            .as_ref()
            .map_or(PromotedGroupFacets::default(), promoted_facets);
        ensure_uniform_aggregation_groups(&spans, &compiled.representation.aggr_methods, promoted)
            .map_err(TemporalStoreError::from)
            .map_err(TemporalError::from)?;

        let mut rows = Vec::with_capacity(pg_rows.len());
        for (pg_row, span) in pg_rows.iter().zip(&spans) {
            let mut row = decode_aggregation_row(pg_row, span).map_err(TemporalError::from)?;
            if let Some(grouping) = &compiled.representation.grouping {
                row.group_keys =
                    decode_group_keys(pg_row, grouping, entity_id, row.dataset_id.as_ref(), row.attribute_type, span.units).map_err(TemporalError::from)?;
            }
            rows.push(row);
        }

        Ok(rows)
    }
}
