//! Deciding the value, geo and scope filters of a split temporal query over
//! the gathered parts of every entity a Context Source contributed to (clause
//! 5.7.4.4).

use crate::{answered_evolution::AnsweredEvolution, error::TemporalError, port::read_scope::ScopeReach, service::TemporalService};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::{
    queried_entity::linked_entities::NoLinkedEntities,
    split_filter::{LeafTruths, SplitFilter},
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use std::collections::HashSet;

/// Which of the contributed entities the filters keep.
#[derive(Debug, Default)]
pub struct SplitSettlement {
    /// The contributed entities whose gathered parts satisfy the filters.
    pub retained: HashSet<Identifier>,
}

impl TemporalService {
    /// Decide the value, geo and scope filters of `params` over every entity
    /// the Context Sources `answered` with, each judged on its part held here
    /// together with every part a source answered.
    ///
    /// Clause 5.7.4.4 applies the filters "after the Entity information from
    /// different Context Sources and local information, if there is any, has
    /// been aggregated". Every leaf of them is existential over the instances it
    /// reads, so a leaf holds over the gathered entity when it holds over any
    /// part, and the filters are decided over the leaves joined across parts.
    /// The local part is read by the store, over the window and the clause 4.11
    /// instance in effect when it opens; an answered part is read as the
    /// Temporal Evolution it is.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the query does not compile, the context
    /// does not resolve, or the local parts cannot be read.
    pub async fn settle_split_filters(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        context: &ContextSpec,
        answered: &[&TemporalDocument],
    ) -> Result<SplitSettlement, TemporalError> {
        let compiled = self.compile_temporal_query(params, context).await?;
        let filter = SplitFilter::of(&compiled.entities);
        let parts: Vec<(Identifier, &TemporalDocument)> = answered.iter().filter_map(|answer| Some((answer.id()?.clone(), *answer))).collect(); // clone: the part keeps the identifier the entity is judged under
        let mut entity_ids: Vec<Identifier> = parts.iter().map(|(entity_id, _)| entity_id.clone()).collect(); // clone: the store reads the identifiers the parts keep
        entity_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        entity_ids.dedup();
        if filter.is_empty() {
            return Ok(SplitSettlement {
                retained: entity_ids.into_iter().collect(),
            });
        }

        let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
        let mut truths = self.reader.filter_leaf_truths(&scope, &compiled, &entity_ids).await?;
        for (entity_id, answer) in parts {
            truths
                .entry(entity_id)
                .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                // A hop leaf is decided by the store over the local part
                // alone; an answered part names linked entities this broker
                // reads nothing of here.
                .or(&filter.truths_of(&AnsweredEvolution::of(answer), NoLinkedEntities));
        }
        let retained = entity_ids
            .into_iter()
            .filter(|entity_id| truths.get(entity_id).is_some_and(|held| filter.holds(held)))
            .collect();
        Ok(SplitSettlement { retained })
    }
}
