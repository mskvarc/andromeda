//! Reading the truth of every leaf of a split temporal query's filters over
//! the part of each contributed entity this store holds (clause 5.7.4.4).

use crate::{error::TemporalStoreError, repo::TemporalRepository, stored_catalog_error::StoredCatalogError, temporal_query_execution::TemporalStatement};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    split_filter::{LeafTruths, SplitFilter},
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_postgresql_temporal_query::split_filter_leaves::build_filter_leaf_truths;
use andromeda_temporal::error::TemporalError;
use sqlx::{Row, postgres::PgRow};
use std::collections::HashMap;

impl TemporalRepository {
    /// The truth of every leaf of the query's filters over the part of each
    /// of `entity_ids` this store holds; an entity it holds no Temporal
    /// Evolution of is absent.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a name lookup, the rendering or the
    /// statement fails, or a row does not read back.
    pub(crate) async fn read_filter_leaf_truths(
        &self,
        branch: &HistoryBranch,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, LeafTruths>, TemporalError> {
        let filter = SplitFilter::of(&compiled.entities);
        if entity_ids.is_empty() || filter.is_empty() {
            return Ok(HashMap::new());
        }
        let names = self.resolve_query_names(compiled).await?;
        let statement = build_filter_leaf_truths(branch, compiled, entity_ids, &names).map_err(TemporalStoreError::Render)?;
        let rows = self
            .execute_temporal_query(TemporalStatement::SplitFilterLeaves, &statement, StatementPlanning::Cached)
            .await?;
        let leaves = filter.leaf_count();
        rows.iter()
            .map(|row| decode_leaf_row(row, leaves))
            .collect::<Result<_, TemporalStoreError>>()
            .map_err(TemporalError::from)
    }
}

/// One entity's identifier and the truths of its `leaves` leaf columns.
///
/// A leaf that renders to SQL `NULL` -- a comparison against a value the
/// instance does not hold -- did not hold.
fn decode_leaf_row(row: &PgRow, leaves: usize) -> Result<(Identifier, LeafTruths), TemporalStoreError> {
    let raw: String = row.try_get(0)?;
    let entity_id = raw.parse::<Identifier>().map_err(StoredCatalogError::EntityId)?;
    let truths = (1..=leaves)
        .map(|column| Ok(row.try_get::<Option<bool>, _>(column)?.unwrap_or(false)))
        .collect::<Result<Vec<bool>, TemporalStoreError>>()?;
    Ok((entity_id, LeafTruths::from_vec(truths)))
}
