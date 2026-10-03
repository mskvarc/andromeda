//! Deciding the value, geo and scope filters of a split temporal query over
//! the gathered parts of every entity a Context Source contributed to (clause
//! 5.7.4.4).

use crate::{
    answer_narrowing::AnswerNarrowing,
    answered_evolution::AnsweredEvolution,
    error::TemporalError,
    service::{TemporalQueryWarning, TemporalService},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::split_filter::{LeafTruths, SplitFilter};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use serde_json::Value;
use std::collections::HashSet;

/// Which of the contributed entities the filters keep.
#[derive(Debug, Default)]
pub struct SplitSettlement {
    /// The contributed entities whose gathered parts satisfy the filters.
    pub retained: HashSet<Identifier>,
    /// One warning per answered part that could not be read, which therefore
    /// decided nothing about its entity (clause 6.3.17).
    pub warnings: Vec<TemporalQueryWarning>,
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
    /// Temporal Evolution it is. An answered part that cannot be read decides
    /// nothing and is reported as a warning, so the entity stands or falls by
    /// its other parts.
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
        answered: &[&Value],
    ) -> Result<SplitSettlement, TemporalError> {
        let compiled = self.compile_temporal_query(params, context).await?;
        let filter = SplitFilter::of(&compiled.entities);
        let parts: Vec<(Identifier, &Value)> = answered.iter().filter_map(|answer| Some((answered_identifier(answer)?, *answer))).collect();
        let mut entity_ids: Vec<Identifier> = parts.iter().map(|(entity_id, _)| entity_id.clone()).collect(); // clone: the store reads the identifiers the parts keep
        entity_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        entity_ids.dedup();
        if filter.is_empty() {
            return Ok(SplitSettlement {
                retained: entity_ids.into_iter().collect(),
                warnings: Vec::new(),
            });
        }

        let mut truths = self.reader.filter_leaf_truths(target, &compiled, &entity_ids).await?;
        let expansion = EntityExpansion::prepare(&self.engine, context).await?;
        let prepared = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared);
        let mut narrowing = AnswerNarrowing::new(&compiled.entities, compiled.window.timeproperty, &expander);
        let mut warnings = Vec::new();
        for (entity_id, answer) in parts {
            match AnsweredEvolution::read(&expansion, &mut narrowing, answer, self.limits.payload_budget).await {
                Ok(part) => truths
                    .entry(entity_id)
                    .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                    .or(&filter.truths_of(&part)),
                Err(error) => warnings.push(TemporalQueryWarning {
                    entity_id,
                    message: format!("a context source's part of the entity could not be read, so it decided no filter: {error}"),
                }),
            }
        }
        let retained = entity_ids
            .into_iter()
            .filter(|entity_id| truths.get(entity_id).is_some_and(|held| filter.holds(held)))
            .collect();
        Ok(SplitSettlement { retained, warnings })
    }
}

/// The identifier an answered entity names, when it names a valid one; an
/// answer naming none is no part of any entity.
fn answered_identifier(answer: &Value) -> Option<Identifier> {
    answer.get("id")?.as_str()?.parse().ok()
}
