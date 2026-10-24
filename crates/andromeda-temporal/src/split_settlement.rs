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
//!
//! A Query Term crossing a Relationship hop reaches entities any part may name
//! and any broker may hold, so every part, this broker's Relationship
//! instances inside the window among them, is judged in memory over the
//! targets the hops reach, each read over the window.

use crate::{
    answered_evolution::AnsweredEvolution,
    error::TemporalError,
    hop_targets::HopTargets,
    in_effect_entity::InEffectEntity,
    local_in_effect::{LocalInEffect, part_document},
    port::read_scope::ScopeReach,
    query_reading::QueryReading,
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::split_filter::{LeafTruths, SplitFilter};
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
    /// The request's terms and `q` target languages are read as `reading`
    /// says.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the query does not compile, the context
    /// does not resolve, or the local parts cannot be read.
    pub async fn settle_split_filters(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        reading: QueryReading<'_>,
        answered: &AnsweredParts<'_>,
        local: &LocalInEffect,
    ) -> Result<SplitSettlement, TemporalError> {
        let compiled = self.compile_temporal_query(params, reading.context, reading.language_targeting).await?;
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
        let hops = filter.hop_relationships();
        let local_links = if hops.is_empty() {
            Vec::new()
        } else {
            let rows = self.reader.window_relationship_instances(&scope, &compiled, &entity_ids).await?;
            rows.into_iter()
                .map(|(entity_id, rows)| part_document(&entity_id, rows))
                .collect::<Result<Vec<_>, _>>()?
        };
        let windowed: Vec<&TemporalDocument> = answered.in_window.iter().copied().chain(&local_links).collect();
        let targets = if hops.is_empty() {
            Vec::new()
        } else {
            let reached: Vec<&TemporalDocument> = windowed
                .iter()
                .copied()
                .chain(answered.in_effect.iter().copied())
                .chain(local.parts())
                .collect();
            self.read_hop_targets(target, params, reading.context, &hops, &reached).await?
        };
        let linked = HopTargets::of(&targets, compiled.window);
        for part in windowed {
            let Some(entity_id) = part.id() else {
                continue;
            };
            truths
                .entry(entity_id.clone()) // clone: the truths are keyed by the identifier the part is judged under
                .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                .or(&filter.truths_of(&AnsweredEvolution::of(part, compiled.window), &linked));
        }
        for (entity_id, mut held) in parts {
            held.extend(local.part_of(entity_id));
            let Some(in_effect) = InEffectEntity::merged(&held, compiled.window) else {
                continue;
            };
            truths
                .entry(entity_id.clone()) // clone: the truths are keyed by the identifier the parts are judged under
                .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                .or(&filter.truths_of(&in_effect, &linked));
        }
        let retained = entity_ids
            .into_iter()
            .filter(|entity_id| truths.get(entity_id).is_some_and(|held| filter.holds(held)))
            .collect();
        Ok(SplitSettlement { retained })
    }
}

#[cfg(test)]
mod tests;
