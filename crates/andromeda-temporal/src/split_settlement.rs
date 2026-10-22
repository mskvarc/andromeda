//! Deciding the value, geo and scope filters of a split temporal query over
//! the gathered parts of every entity a Context Source contributed to (clause
//! 5.7.4.4).
//!
//! Clause 5.7.4.4 applies the filters "after the Entity information from
//! different Context Sources and local information, if there is any, has been
//! aggregated". Every leaf of them is existential over the instances valid
//! within the window (clause 4.11), which are of two kinds. Over the instances
//! inside the window a leaf holds over the gathered entity when it holds over
//! any part, so each part is judged on its own and the truths are joined: the
//! local part by the store, an answered part as the Temporal Evolution it is.
//! The instance each stream held when the window opens is the latest across
//! every part, which no part decides alone and which a newer deletion in one
//! part ends, so the in-effect instances of every part are merged first and
//! the merged entity is judged once. A leaf holds when either kind holds it.

use crate::{
    answered_evolution::AnsweredEvolution,
    error::TemporalError,
    in_effect_entity::InEffectEntity,
    local_in_effect::LocalInEffect,
    port::read_scope::ScopeReach,
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::{
    queried_entity::linked_entities::NoLinkedEntities,
    split_filter::{LeafTruths, SplitFilter},
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use std::collections::{HashMap, HashSet};

/// Which of the contributed entities the filters keep.
#[derive(Debug, Default)]
pub struct SplitSettlement {
    /// The contributed entities whose gathered parts satisfy the filters.
    pub retained: HashSet<Identifier>,
}

/// What the Context Sources answered a split query with.
#[derive(Debug, Default)]
pub struct AnsweredParts<'a> {
    /// The parts answered for the window.
    pub in_window: Vec<&'a TemporalDocument>,
    /// The parts answered for the instances in effect when the window opens.
    pub in_effect: Vec<&'a TemporalDocument>,
}

impl TemporalService {
    /// Decide the value, geo and scope filters of `params` over every entity
    /// the Context Sources `answered` with, each judged on its part held here,
    /// read over the window, beside `local`, what this broker holds in effect
    /// when the window opens, together with every part a source answered.
    ///
    /// A Query Term crossing a Relationship hop reads its linked entities in
    /// the store alone, so it is decided over the local part, which the store
    /// reads over the window and its own instance in effect.
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
        answered: &AnsweredParts<'_>,
        local: &LocalInEffect,
    ) -> Result<SplitSettlement, TemporalError> {
        let compiled = self.compile_temporal_query(params, context).await?;
        let filter = SplitFilter::of(&compiled.entities);
        let mut parts: HashMap<&Identifier, Vec<&TemporalDocument>> = HashMap::new();
        for part in answered.in_window.iter().chain(&answered.in_effect) {
            if let Some(entity_id) = part.id() {
                parts.entry(entity_id).or_default().push(part);
            }
        }
        let mut entity_ids: Vec<Identifier> = parts.keys().map(|entity_id| (*entity_id).clone()).collect(); // clone: the store reads the identifiers the parts keep
        entity_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        if filter.is_empty() {
            return Ok(SplitSettlement {
                retained: entity_ids.into_iter().collect(),
            });
        }

        let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
        let mut truths = self.reader.filter_leaf_truths(&scope, &compiled, &entity_ids).await?;
        for answer in &answered.in_window {
            let Some(entity_id) = answer.id() else {
                continue;
            };
            truths
                .entry(entity_id.clone()) // clone: the truths are keyed by the identifier the part is judged under
                .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                .or(&filter.truths_of(&AnsweredEvolution::of(answer, compiled.window), NoLinkedEntities));
        }
        for (entity_id, mut held) in parts {
            held.extend(local.part_of(entity_id));
            let Some(in_effect) = InEffectEntity::merged(&held, compiled.window) else {
                continue;
            };
            truths
                .entry(entity_id.clone()) // clone: the truths are keyed by the identifier the parts are judged under
                .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                .or(&filter.truths_of(&in_effect, NoLinkedEntities));
        }
        let retained = entity_ids
            .into_iter()
            .filter(|entity_id| truths.get(entity_id).is_some_and(|held| filter.holds(held)))
            .collect();
        Ok(SplitSettlement { retained })
    }
}
