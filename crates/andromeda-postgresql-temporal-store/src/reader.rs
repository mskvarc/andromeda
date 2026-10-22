//! Stage 1 resolves entity ids, including the `q`-attribute dictionary lookup; stage 2 runs the per-entity history SQL builder and decodes each `PgRow` into a [`TemporalHistoryRow`].

use crate::{
    aggregation_rows_read::decode_aggregation_rows,
    error::TemporalStoreError,
    history_branch::branch_of,
    history_row_decode::assembled_history,
    repo::TemporalRepository,
    scope_history_read::decode_scope_history,
    stored_attribute_shape::StoredAttributeShape,
    stored_catalog_error::StoredCatalogError,
    temporal_query_execution::TemporalStatement,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::PageWindow,
    split_filter::LeafTruths,
    temporal::{
        compiled::CompiledTemporalQuery,
        grouping::{AggregationGrouping, group_key::GroupKey},
    },
};
use andromeda_postgresql_codes::{
    attribute_kind_code::{attribute_type_of, stored_attribute_kind},
    recorded_shape::RecordedShape,
};
use andromeda_postgresql_query::{
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    history_source::HistoryBranch,
    projected_attribute_names::projected_attribute_names,
    projected_type_names::projected_type_names,
};
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_postgresql_temporal_query::{
    history::build_temporal_history_query,
    scope_history::{build_scope_history_query, scope_is_selected},
    stage1::build_stage1_entity_ids,
};
use andromeda_temporal::{
    error::TemporalError,
    outcome::TemporalEntityIdOutcome,
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            RecordedAttributeShape,
            RecordedValueKind,
            ShapeHolders,
            TemporalHistoryObservation,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        read_scope::{ScopeReach, TemporalReadScope},
    },
};
use async_trait::async_trait;
use iri_rs::IriBuf;
use sqlx::{Row, postgres::PgRow};
use std::collections::{HashMap, HashSet};

/// The clause 6.3.13 total the stage 1 count statement answered.
///
/// The statement aggregates without grouping, so it answers exactly one row:
/// no row is a broken contract, and a `count` that does not decode is a fault
/// rather than an absent total, which would answer the page without the
/// `NGSILD-Results-Count` the client asked for.
fn stage1_total(rows: &[PgRow]) -> Result<i64, TemporalStoreError> {
    let row = rows.first().ok_or(TemporalStoreError::Invariant(TemporalStoreInvariant::CountRowMissing))?;
    Ok(row.try_get("count")?)
}

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

/// The stored [`RecordedShape`] one neutral value shape is projected as: the
/// inverse of [`recorded_value_kind`].
const fn stored_recorded_shape(kind: RecordedValueKind) -> RecordedShape {
    match kind {
        RecordedValueKind::Number => RecordedShape::Number,
        RecordedValueKind::String => RecordedShape::String,
        RecordedValueKind::Boolean => RecordedShape::Boolean,
        RecordedValueKind::Geometry => RecordedShape::Geometry,
        RecordedValueKind::Object => RecordedShape::Object,
        RecordedValueKind::Array => RecordedShape::Array,
        RecordedValueKind::DateTime => RecordedShape::DateTime,
        RecordedValueKind::Date => RecordedShape::Date,
        RecordedValueKind::Time => RecordedShape::Time,
        RecordedValueKind::Uri => RecordedShape::Uri,
    }
}

/// The stored family-and-value-shape pair one neutral shape is recorded as:
/// the inverse of [`neutral_attribute_shape`].
pub(crate) const fn stored_attribute_shape(neutral: RecordedAttributeShape) -> StoredAttributeShape {
    StoredAttributeShape {
        kind: stored_attribute_kind(neutral.family),
        shape: stored_recorded_shape(neutral.value_kind),
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
    /// Resolve the attribute and type names a compiled query's selection reads
    /// through the dictionaries the write path minted their keys in.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a dictionary lookup fails.
    pub(crate) async fn resolve_query_names(&self, compiled: &CompiledTemporalQuery) -> Result<QueryNames, TemporalError> {
        // Clause 5.7.4.3 confines a temporal ordering to the `id` member, which
        // is a column of the temporal catalogue and names no attribute, so the
        // dictionary lookup below has no ordering term to resolve. The `attrs`
        // selection, the `pick`/`omit` projection and the attributes an
        // `aggrGroupBy` keys by resolve with the filters, so a statement
        // narrows the history it reads by key inside the source rather than by
        // name above it.
        let grouped = compiled
            .representation
            .grouping
            .iter()
            .flat_map(AggregationGrouping::terms)
            .filter_map(|term| match &term.key {
                GroupKey::Path(path) => Some(path.attr.as_str()),
                GroupKey::Member(_) => None,
            });
        let attribute_names = projected_attribute_names(
            compiled
                .instances
                .attr_filter
                .iter()
                .map(IriBuf::as_str)
                .chain(compiled.entities.projected_members.names())
                .chain(grouped),
            compiled.entities.q_expr.as_ref(),
            compiled.entities.geo.as_ref(),
            &[],
        );
        // The projection is written inside the transaction that writes the
        // entity, so a dictionary hit is already backed by committed projection
        // rows and there is nothing to wait for.
        let attributes = if attribute_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.attribute_names.lookup(&attribute_names).await.map_err(TemporalStoreError::from)?
        };
        // The catalogue carries type keys, so a type selection resolves through
        // the same dictionary the write path minted them in.
        let type_names = projected_type_names(&compiled.entities);
        let types = if type_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.entity_type_names.lookup(&type_names).await.map_err(TemporalStoreError::from)?
        };
        Ok(QueryNames { attributes, types })
    }

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
        branch: &HistoryBranch,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalHistoryObservation>, TemporalError> {
        if !scope_is_selected(compiled) {
            return Ok(Vec::new());
        }
        let statement = build_scope_history_query(branch, entity_id, compiled);
        let pg_rows = self
            .execute_temporal_query(TemporalStatement::ScopeHistory, &statement, StatementPlanning::Cached)
            .await?;
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

    async fn read_scope<'a>(&self, target: &'a ContextTarget, reach: ScopeReach<'_>) -> Result<TemporalReadScope<'a>, TemporalError> {
        self.scope_of(target, reach).await.map_err(TemporalError::from)
    }

    async fn entity_exists(&self, scope: &TemporalReadScope<'_>, entity_id: &Identifier) -> Result<bool, TemporalError> {
        self.temporal_entity_exists(&branch_of(scope), entity_id).await.map_err(TemporalError::from)
    }

    async fn entity_types(&self, scope: &TemporalReadScope<'_>, entity_ids: &[Identifier]) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError> {
        self.temporal_entity_types(&branch_of(scope), entity_ids).await.map_err(TemporalError::from)
    }

    async fn recorded_attribute_shapes(
        &self,
        scope: &TemporalReadScope<'_>,
        attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        let recorded = self.stored_attribute_shapes(&branch_of(scope), attributes).await.map_err(TemporalError::from)?;
        Ok(recorded
            .into_iter()
            .map(|(name, shapes)| (name, shapes.into_iter().map(neutral_attribute_shape).collect()))
            .collect())
    }

    async fn held_attribute_shapes(
        &self,
        scope: &TemporalReadScope<'_>,
        holders: ShapeHolders<'_>,
        compiled: &CompiledTemporalQuery,
        shapes: &HashMap<AttributeName, HashSet<RecordedAttributeShape>>,
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        self.held_shapes(&branch_of(scope), holders, compiled, shapes).await
    }

    async fn query_entity_ids(
        &self,
        scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        window: &PageWindow,
        count: CountPolicy,
    ) -> Result<TemporalEntityIdOutcome, TemporalError> {
        let names = self.resolve_query_names(compiled).await?;

        // The statement compares identifiers under the collation the ordering
        // names, and PostgreSQL pre-imports only the `<locale>-x-icu` forms; a
        // keyword form clause 4.23.3 demonstrates has no object until one is
        // created, and ordering under it would fail the statement outright.
        if let Some(collation) = compiled.ordering.as_ref().and_then(|ordering| ordering.collation.as_ref()) {
            self.collations.ensure(collation).await.map_err(TemporalStoreError::from)?;
        }

        let stage1 = build_stage1_entity_ids(&branch_of(scope), compiled, window, count, &names).map_err(TemporalStoreError::Render)?;

        let entity_id_rows = self.execute_temporal_query(TemporalStatement::Stage1, &stage1.select, stage1.planning).await?;

        let mut entity_ids = entity_id_rows
            .into_iter()
            .map(|row| {
                let raw: String = row.try_get("entity_id")?;
                Ok(raw.parse::<Identifier>().map_err(StoredCatalogError::EntityId)?)
            })
            .collect::<Result<Vec<_>, TemporalStoreError>>()?;
        // Clause 5.5.9.1: a backward or from-end keyset page was read descending
        // so one index scan could serve it; present it ascending, the order every
        // element page shows.
        stage1.read_order.present(&mut entity_ids);
        let entity_ids = entity_ids;

        let total_count = if count == CountPolicy::Emit {
            let count = stage1
                .count
                .ok_or(TemporalStoreError::Invariant(TemporalStoreInvariant::CountStatementMissing))?;
            let count_rows = self
                .execute_temporal_query(TemporalStatement::Stage1Count, &count, stage1.count_planning)
                .await?;
            Some(stage1_total(&count_rows)?)
        } else {
            None
        };

        Ok(TemporalEntityIdOutcome {
            entity_ids,
            total: total_count,
        })
    }

    async fn filter_leaf_truths(
        &self,
        scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, LeafTruths>, TemporalError> {
        self.read_filter_leaf_truths(&branch_of(scope), compiled, entity_ids).await
    }

    async fn in_effect_instances(
        &self,
        scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        self.read_in_effect_instances(&branch_of(scope), compiled, entity_ids).await
    }

    async fn query_entity_history(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        let branch = branch_of(scope);
        let names = self.resolve_query_names(compiled).await?;
        let stage2 = build_temporal_history_query(&branch, entity_id, compiled, &names);
        let pg_rows = self
            .execute_temporal_query(TemporalStatement::History, &stage2, StatementPlanning::Cached)
            .await?;

        // Clause 4.18 reifies the Scope in the temporal representation, and it
        // is stored apart from attribute history because clause 4.5.4 gives it
        // no attribute stream, so the representation is assembled from two
        // reads. The scope leads, which is the order annex C clause C.5.16
        // renders it in.
        let mut observations = self.read_scope_history(&branch, entity_id, compiled).await?;
        observations.extend(
            assembled_history(&pg_rows, self.attribute_names.as_ref(), self.entity_type_names.as_ref())
                .await
                .map_err(TemporalError::from)?,
        );

        Ok(TemporalHistoryOutcome { observations })
    }

    async fn query_merged_entity_aggregation(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        answered: &[TemporalHistoryRow],
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        self.merged_entity_aggregation(scope.target(), entity_id, entity_types, compiled, answered)
            .await
    }

    async fn query_entity_aggregation(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        let names = self.resolve_query_names(compiled).await?;
        let stage2 = build_temporal_history_query(&branch_of(scope), entity_id, compiled, &names);
        let pg_rows = self
            .execute_temporal_query(TemporalStatement::Aggregation, &stage2, StatementPlanning::Cached)
            .await?;

        decode_aggregation_rows(&pg_rows, entity_id, compiled)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TemporalStoreError,
        reader::{neutral_attribute_shape, stage1_total, stored_attribute_shape},
        stored_attribute_shape::StoredAttributeShape,
        temporal_store_invariant::TemporalStoreInvariant,
    };
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, recorded_shape::RecordedShape};

    #[test]
    fn a_count_statement_answering_no_row_is_a_broken_contract() {
        assert!(matches!(
            stage1_total(&[]),
            Err(TemporalStoreError::Invariant(TemporalStoreInvariant::CountRowMissing))
        ));
    }

    /// A shape looked for among the instances of a request is the one the
    /// stream catalogue reported, so every stored pair survives the trip to
    /// the port and back.
    #[test]
    fn every_stored_shape_round_trips_through_its_neutral_shape() {
        for kind in AttributeKind::ALL {
            for shape in RecordedShape::ALL {
                let stored = StoredAttributeShape { kind: *kind, shape: *shape };
                assert_eq!(stored_attribute_shape(neutral_attribute_shape(stored)), stored);
            }
        }
    }
}
