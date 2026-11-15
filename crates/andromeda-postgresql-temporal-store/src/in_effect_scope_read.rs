//! Reading the Scope event in force when a split temporal query's window
//! opens, over the part of each contributed entity this store holds (clauses
//! 4.18 and 5.7.4.4).

use crate::{entity_history_runs::by_entity, repo::TemporalRepository, scope_history_read::decode_scope_history};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_postgresql_temporal_query::split_part_rows::build_in_effect_scope;
use andromeda_telemetry::statement::ObservedStatement;
use andromeda_temporal::error::TemporalError;
use std::collections::HashMap;

impl TemporalRepository {
    /// The Scope event in force when the query's window opens, in the part of
    /// each of `entity_ids` this store holds; an entity holding none is
    /// absent.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the statement fails or a row does not
    /// read back.
    pub(crate) async fn read_in_effect_scope(
        &self,
        branch: &HistoryBranch,
        compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        let mut in_effect: HashMap<Identifier, Vec<TemporalHistoryRow>> = HashMap::new();
        if entity_ids.is_empty() {
            return Ok(in_effect);
        }
        let Some(statement) = build_in_effect_scope(branch, compiled, entity_ids) else {
            return Ok(in_effect);
        };
        let rows = self
            .execute_temporal_query(ObservedStatement::TemporalSourceWindowStartScope, &statement, StatementPlanning::Cached)
            .await?;
        for (entity_id, entity_rows) in by_entity(&rows)? {
            let events = decode_scope_history(entity_rows)?;
            in_effect.entry(entity_id).or_default().extend(events.into_iter().map(|event| event.row));
        }
        Ok(in_effect)
    }
}
