//! Reading the instances a split temporal query's filters read in effect when
//! its window opens, over the part of each contributed entity this store holds
//! (clauses 4.11 and 5.7.4.4).

use crate::{
    error::TemporalStoreError,
    history_row_decode::assembled_history,
    repo::TemporalRepository,
    scope_history_read::decode_scope_history,
    stored_catalog_error::StoredCatalogError,
    temporal_query_execution::TemporalStatement,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_ngsi_ld_query::{split_filter::SplitFilter, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_postgresql_temporal_query::in_effect_instances::{IN_EFFECT_ENTITY_ID, build_in_effect_attribute_instances, build_in_effect_scope};
use andromeda_temporal::error::TemporalError;
use sqlx::{Row, postgres::PgRow};
use std::collections::HashMap;

impl TemporalRepository {
    /// The instances the query's filters read in effect when its window
    /// opens, in the part of each of `entity_ids` this store holds, the Scope
    /// event in force first; an entity holding none is absent.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a name lookup or a statement fails, or a
    /// row does not read back.
    pub(crate) async fn read_in_effect_instances(
        &self,
        branch: &HistoryBranch,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        let filter = SplitFilter::of(&compiled.entities);
        let mut in_effect: HashMap<Identifier, Vec<TemporalHistoryRow>> = HashMap::new();
        if entity_ids.is_empty() {
            return Ok(in_effect);
        }
        if filter.reads_scope()
            && let Some(statement) = build_in_effect_scope(branch, compiled, entity_ids)
        {
            let rows = self
                .execute_temporal_query(TemporalStatement::InEffectScope, &statement, StatementPlanning::Cached)
                .await?;
            for (entity_id, entity_rows) in by_entity(&rows)? {
                let events = decode_scope_history(entity_rows)?;
                in_effect.entry(entity_id).or_default().extend(events.into_iter().map(|event| event.row));
            }
        }
        let names = self.resolve_query_names(compiled).await?;
        let keys = names
            .attributes
            .keys_of(filter.read_attributes().into_iter().map(|attribute| attribute.as_str()));
        if keys.is_empty() {
            return Ok(in_effect);
        }
        let Some(statement) = build_in_effect_attribute_instances(branch, compiled, entity_ids, &keys) else {
            return Ok(in_effect);
        };
        let rows = self
            .execute_temporal_query(TemporalStatement::InEffectInstances, &statement, StatementPlanning::Cached)
            .await?;
        let entities = rows.iter().map(entity_of).collect::<Result<Vec<_>, _>>()?;
        let observations = assembled_history(&rows, self.attribute_names.as_ref(), self.entity_type_names.as_ref()).await?;
        for (entity_id, observation) in entities.into_iter().zip(observations) {
            in_effect.entry(entity_id).or_default().push(observation.row);
        }
        Ok(in_effect)
    }
}

/// The entity a row was read for.
fn entity_of(row: &PgRow) -> Result<Identifier, TemporalStoreError> {
    let raw: String = row.try_get(IN_EFFECT_ENTITY_ID)?;
    Ok(raw.parse::<Identifier>().map_err(StoredCatalogError::EntityId)?)
}

/// `rows`, ordered by entity, cut into the runs of each entity.
///
/// A Scope event is folded from the rows it landed, so the rows of one
/// entity are decoded apart from every other entity's.
fn by_entity(rows: &[PgRow]) -> Result<Vec<(Identifier, &[PgRow])>, TemporalStoreError> {
    let entities = rows.iter().map(entity_of).collect::<Result<Vec<_>, _>>()?;
    let mut runs: Vec<(Identifier, &[PgRow])> = Vec::new();
    let mut start = 0;
    for end in 1..=rows.len() {
        if entities.get(end) != entities.get(start)
            && let (Some(entity_id), Some(run)) = (entities.get(start), rows.get(start..end))
        {
            runs.push((entity_id.clone(), run)); // clone: each run owns the identifier it is keyed by
            start = end;
        }
    }
    Ok(runs)
}
