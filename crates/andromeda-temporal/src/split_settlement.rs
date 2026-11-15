//! Deciding the value, geo and scope filters of a split temporal query over
//! the gathered parts of every entity a Context Source contributed to (clause
//! 5.7.4.4).
//!
//! Clause 5.7.4.4 applies the filters "after the Entity information from
//! different Context Sources and local information, if there is any, has been
//! aggregated". Every leaf of the value and geo filters is existential over the
//! instances inside the window (steps S2 and S3), so it holds over the
//! gathered entity when it holds over any part: each part is judged on its own
//! and the truths are joined, the local part by the store, an answered part as
//! the Temporal Evolution it is.
//!
//! A scope requirement also reads the Scope in force when the window opens
//! (clause 4.18, Annex C clause C.5.16). That is the latest Scope event across
//! every part, which no part decides alone and which a newer removal in one
//! part ends, so the in-force Scope events of every part are merged first and
//! the merged entity is judged once. A requirement holds when either reading
//! holds it.
//!
//! A Query Term crossing a Relationship hop reaches entities any part may name
//! and any broker may hold, so every part, this broker's Relationship
//! instances inside the window among them, is judged in memory over the
//! targets the hops reach, each read over the window.

use crate::{
    answered_evolution::AnsweredEvolution,
    error::TemporalError,
    hop_targets::{HopReading, HopTargets},
    in_effect_scope::InEffectScope,
    local_in_effect::{LocalInEffect, part_document},
    port::read_scope::ScopeReach,
    query_reading::QueryReading,
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::{
    projection::attrs_selection::AttrsAdmission,
    split_filter::{LeafTruths, SplitFilter},
};
use andromeda_ngsi_ld_query_evaluation::split_filter::leaf_truths;
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use andromeda_ngsi_ld_request::query::temporal::admitted::AdmittedTemporalRequest;
use std::collections::{HashMap, HashSet};

/// Which of the contributed entities the filters keep.
#[derive(Debug, Default)]
pub struct SplitSettlement {
    /// The contributed entities whose gathered parts satisfy the filters.
    pub retained: HashSet<Identifier>,
    /// The holders of a hop target that could not be read, whose parts the
    /// filters were decided without.
    pub incomplete: Vec<LinkedResolutionFailure>,
}

/// What the Context Sources answered a split query with.
#[derive(Debug, Default)]
pub struct AnsweredParts<'a> {
    /// The parts answered for the window.
    pub in_window: Vec<&'a TemporalDocument>,
    /// The parts answered for the Scope in force when the window opens.
    pub in_effect: Vec<&'a TemporalDocument>,
}

impl TemporalService {
    /// Decide the value, geo and scope filters of `params` over every entity
    /// the Context Sources `answered` with, each judged on its part held here,
    /// read over the window, beside `local`, the Scope this broker holds in
    /// force when the window opens, together with every part a source
    /// answered.
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
        params: &AdmittedTemporalRequest,
        reading: QueryReading<'_>,
        answered: &AnsweredParts<'_>,
        local: &LocalInEffect,
    ) -> Result<SplitSettlement, TemporalError> {
        let compiled = self.compile_temporal_query(params, reading, AttrsAdmission::ReifiedScopeAdmitted).await?;
        let filter = SplitFilter::of(&compiled.entities);
        let mut parts: HashMap<&Identifier, Vec<&TemporalDocument>> = HashMap::new();
        for part in answered.in_window.iter().chain(&answered.in_effect) {
            if let Some(entity_id) = part.id() {
                parts.entry(entity_id).or_default().push(part);
            }
        }
        let mut entity_ids: Vec<Identifier> = parts.keys().map(|entity_id| (*entity_id).clone()).collect();
        entity_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        if filter.is_empty() {
            return Ok(SplitSettlement {
                retained: entity_ids.into_iter().collect(),
                incomplete: Vec::new(),
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
        let hop_reading = if hops.is_empty() {
            HopReading::default()
        } else {
            self.read_hop_targets(target, params, reading, (&hops, &filter.hop_reads()), &windowed).await?
        };
        let linked = HopTargets::of(&hop_reading, compiled.window);
        for part in windowed {
            let Some(entity_id) = part.id() else {
                continue;
            };
            truths
                .entry(entity_id.clone())
                .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                .or(&leaf_truths(&filter, &AnsweredEvolution::of(part, compiled.window), &linked));
        }
        for (entity_id, mut held) in parts.into_iter().filter(|_| filter.reads_scope()) {
            held.extend(local.part_of(entity_id));
            let Some(in_effect) = InEffectScope::merged(&held, compiled.window) else {
                continue;
            };
            truths
                .entry(entity_id.clone())
                .or_insert_with(|| LeafTruths::none(filter.leaf_count()))
                .or(&leaf_truths(&filter, &in_effect, &linked));
        }
        let retained = entity_ids
            .into_iter()
            .filter(|entity_id| truths.get(entity_id).is_some_and(|held| filter.holds(held)))
            .collect();
        Ok(SplitSettlement {
            retained,
            incomplete: hop_reading.incomplete,
        })
    }
}

#[cfg(test)]
mod tests;
