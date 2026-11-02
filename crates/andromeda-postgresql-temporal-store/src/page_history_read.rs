//! Reading the history, or the aggregates, of every entity of a page in one
//! statement each (clauses 5.7.3 and 5.7.4).
//!
//! The statements read each entity on its own inside them, and the rows come
//! back ordered by entity, so every entity is decoded from its own run: one
//! whose rows do not read back is answered as its failure and the rest of the
//! page still reads.

use crate::{
    aggregation_rows_read::{decode_aggregation_rows, decode_period_origin},
    entity_history_runs::{assembled_entity_histories, by_entity},
    error::TemporalStoreError,
    repo::TemporalRepository,
    scope_history_read::decode_scope_history,
    temporal_query_execution::TemporalStatement,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_postgresql_temporal_query::{
    history::build_temporal_history_query,
    scope_history::{build_scope_history_query, scope_is_selected},
};
use andromeda_temporal::{
    error::TemporalError,
    port::history_reader::{TemporalAggregationOutcome, TemporalHistoryObservation, TemporalHistoryOutcome},
};
use std::collections::HashMap;

/// What a read of several entities answered for each of them: its rows, or
/// the failure that kept them from reading back. An entity the read found
/// nothing of is absent.
type EntityAnswers<T> = HashMap<Identifier, Result<T, TemporalError>>;

impl TemporalRepository {
    /// The history of each of `entity_ids` the compiled query selects, the
    /// Scope events of each leading its attribute instances.
    ///
    /// Clause 4.18 reifies the Scope in the temporal representation, and it
    /// is stored apart from attribute history because clause 4.5.4 gives it
    /// no attribute stream, so the representation is assembled from two
    /// reads. The scope leads, which is the order annex C clause C.5.16
    /// renders it in.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a name lookup or a statement fails.
    pub(crate) async fn read_entities_history(
        &self,
        branch: &HistoryBranch,
        entity_ids: &[Identifier],
        compiled: &CompiledTemporalQuery,
    ) -> Result<EntityAnswers<TemporalHistoryOutcome>, TemporalError> {
        let names = self.resolve_query_names(compiled).await?;
        let statement = build_temporal_history_query(branch, entity_ids, compiled, &names);
        let rows = self
            .execute_temporal_query(TemporalStatement::History, &statement, StatementPlanning::Cached)
            .await?;
        let mut answers = self.read_entities_scope_history(branch, entity_ids, compiled).await?;
        let histories = assembled_entity_histories(by_entity(&rows)?, self.attribute_names.as_ref(), self.entity_type_names.as_ref()).await?;
        for (entity_id, attributes) in histories {
            let scope = answers.remove(&entity_id).unwrap_or_else(|| Ok(Vec::new()));
            let observations = scope.and_then(|mut observations| {
                observations.extend(attributes?);
                Ok(observations)
            });
            answers.insert(entity_id, observations);
        }
        Ok(answers
            .into_iter()
            .map(|(entity_id, observations)| {
                let outcome = observations.map(|observations| TemporalHistoryOutcome { observations });
                (entity_id, outcome.map_err(TemporalError::from))
            })
            .collect())
    }

    /// The Scope events of each of `entity_ids` the compiled window selects,
    /// none when the request's `attrs` leaves the Scope out.
    ///
    /// Clause 4.18 makes the Scope "a special scope Property that is
    /// non-reified in the normalized Entity representation and reified in the
    /// temporal representation of an Entity", and Table 6.18.3.2-1 lets a
    /// request narrow to it through `attrs`, which annex C clause C.5.16 does
    /// with `attrs=speed,scope`. An `attrs` that names other members alone
    /// therefore skips the read entirely rather than reading rows the response
    /// would drop.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the statement fails or a row does not
    /// name its entity.
    async fn read_entities_scope_history(
        &self,
        branch: &HistoryBranch,
        entity_ids: &[Identifier],
        compiled: &CompiledTemporalQuery,
    ) -> Result<HashMap<Identifier, Result<Vec<TemporalHistoryObservation>, TemporalStoreError>>, TemporalError> {
        if !scope_is_selected(compiled) {
            return Ok(HashMap::new());
        }
        let statement = build_scope_history_query(branch, entity_ids, compiled);
        let rows = self
            .execute_temporal_query(TemporalStatement::ScopeHistory, &statement, StatementPlanning::Cached)
            .await?;
        Ok(by_entity(&rows)?
            .into_iter()
            .map(|(entity_id, run)| (entity_id, decode_scope_history(run)))
            .collect())
    }

    /// The aggregated periods of each of `entity_ids` the compiled query
    /// selects, with the origin each was laid from.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a name lookup or the statement fails.
    pub(crate) async fn read_entities_aggregation(
        &self,
        branch: &HistoryBranch,
        entity_ids: &[Identifier],
        compiled: &CompiledTemporalQuery,
    ) -> Result<EntityAnswers<TemporalAggregationOutcome>, TemporalError> {
        let names = self.resolve_query_names(compiled).await?;
        let statement = build_temporal_history_query(branch, entity_ids, compiled, &names);
        let rows = self
            .execute_temporal_query(TemporalStatement::Aggregation, &statement, StatementPlanning::Cached)
            .await?;
        Ok(by_entity(&rows)?
            .into_iter()
            .map(|(entity_id, run)| {
                let outcome = decode_aggregation_rows(run, &entity_id, compiled).and_then(|periods| {
                    Ok(TemporalAggregationOutcome {
                        periods,
                        origin: decode_period_origin(run)?,
                    })
                });
                (entity_id, outcome)
            })
            .collect())
    }
}
