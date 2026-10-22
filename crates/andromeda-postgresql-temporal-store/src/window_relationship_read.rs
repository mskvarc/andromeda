//! Reading the instances inside a split temporal query's window of the
//! Relationships its Query Terms' hops start from, over the part of each
//! contributed entity this store holds (clauses 4.9 and 5.7.4.4).

use crate::{history_row_decode::assembled_history, in_effect_instance_read::entity_of, repo::TemporalRepository, temporal_query_execution::TemporalStatement};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_ngsi_ld_query::{split_filter::SplitFilter, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_postgresql_temporal_query::split_part_rows::build_window_relationship_instances;
use andromeda_temporal::error::TemporalError;
use std::collections::HashMap;

impl TemporalRepository {
    /// The instances inside the window of the Relationships the first hop of
    /// every Query Term crosses, in the part of each of `entity_ids` this
    /// store holds; an entity holding none is absent.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a name lookup or the statement fails, or
    /// a row does not read back.
    pub(crate) async fn read_window_relationship_instances(
        &self,
        branch: &HistoryBranch,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        let mut instances: HashMap<Identifier, Vec<TemporalHistoryRow>> = HashMap::new();
        let filter = SplitFilter::of(&compiled.entities);
        let Some(first_hops) = filter.hop_relationships().into_iter().next() else {
            return Ok(instances);
        };
        if entity_ids.is_empty() {
            return Ok(instances);
        }
        let names = self.resolve_query_names(compiled).await?;
        let keys = names.attributes.keys_of(first_hops.into_iter().map(AttributeName::as_str));
        if keys.is_empty() {
            return Ok(instances);
        }
        let statement = build_window_relationship_instances(branch, compiled, entity_ids, &keys);
        let rows = self
            .execute_temporal_query(TemporalStatement::WindowRelationships, &statement, StatementPlanning::Cached)
            .await?;
        let entities = rows.iter().map(entity_of).collect::<Result<Vec<_>, _>>()?;
        let observations = assembled_history(&rows, self.attribute_names.as_ref(), self.entity_type_names.as_ref()).await?;
        for (entity_id, observation) in entities.into_iter().zip(observations) {
            instances.entry(entity_id).or_default().push(observation.row);
        }
        Ok(instances)
    }
}
