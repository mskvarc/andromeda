//! Reading the rows of one entity's aggregation statement as the aggregates
//! they compute (clause 4.5.19).

use crate::{
    aggregation_group_key_decode::decode_group_keys,
    aggregation_row_decode::{decode_aggregation_group_span, decode_aggregation_row},
    error::TemporalStoreError,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_temporal_query::{
    aggregation_group::{PromotedGroupFacets, ensure_uniform_aggregation_groups},
    aggregation_grouping::promoted_facets,
};
use andromeda_temporal::error::TemporalError;
use sqlx::postgres::PgRow;

/// The aggregates the rows of `entity_id`'s aggregation statement compute,
/// refused when a group spans inputs of different meaning.
///
/// # Errors
///
/// Returns [`TemporalError`] when a group mixes units, families or value
/// shapes a requested method cannot span, or a row does not read back.
pub(crate) fn decode_aggregation_rows(
    pg_rows: &[PgRow],
    entity_id: &Identifier,
    compiled: &CompiledTemporalQuery,
) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
    // A unit-sensitive aggregate over values that do not share a unit, a family
    // or a shape has no meaning and the broker converts nothing, so the group
    // spans are read and checked before any aggregate is decoded.
    let mut spans = Vec::with_capacity(pg_rows.len());
    for pg_row in pg_rows {
        spans.push(decode_aggregation_group_span(pg_row).map_err(TemporalError::from)?);
    }
    // A facet promoted to an `aggrGroupBy` key splits the groups rather than
    // contradicting them, so the guard skips its uniformity check.
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
