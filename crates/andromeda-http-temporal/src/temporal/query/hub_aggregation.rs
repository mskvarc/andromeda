//! Aggregating at this broker the entities a distributed query meets in more
//! than one part, when the query asks for an aggregated representation and
//! carries no filter decided after the merge (clauses 4.5.19 and 5.7.4.4).
//!
//! Clause 5.7.4.4 merges every holder's part of an entity into the result set
//! and renders "aggregated values of Attribute instances" of the merged
//! entity, so a holder's aggregate of its own part is the entity's aggregate
//! only while no other part exists. An entity held in more than one part is
//! aggregated here over the instances of every part, this broker's own
//! included, and replaces the merge of its holders' aggregates. The
//! deployment's [`AggregationGathering`] decides how those entities are found:
//! every entity a source answered, its instances asked in the one forwarded
//! round, or only the entities more than one part answered, their instances
//! asked in a second round.

use crate::temporal::{
    ProblemResponse,
    ResultInstanceExt,
    TemporalQueryParameters,
    collect_temporal_warnings,
    query::{
        forwarding::{ForwardedHistory, HistoryReach, QueryScope, TemporalForwarding, forward_temporal_history},
        in_effect_gathering::{GatheringRequest, instance_documents},
        merged_history::MergedTemporalHistory,
        split_contribution::ContributionShape,
        whole_range_join::close_hub_aggregates,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{aggregation_gathering::AggregationGathering, outcome::ForwardedTemporalEntities};
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_temporal::{
    merged_parts::{EntityParts, LocalPart},
    query_reading::QueryReading,
    service::TemporalService,
};
use axum::http::Uri;
use std::collections::HashSet;

/// The entities aggregated at this broker, which replace whatever the merge
/// made of their holders' parts.
#[derive(Default)]
pub(crate) struct HubAggregates {
    /// The identifiers of the entities aggregated here.
    ids: HashSet<Identifier>,
    /// Their aggregated representations.
    entities: Vec<TemporalHistory>,
    /// The clause 6.3.17 warnings of the second round, and the entities that
    /// could not be aggregated.
    warnings: Vec<NgsiLdWarning>,
}

impl HubAggregates {
    /// Put the entities aggregated here in place of what the merge made of
    /// them.
    pub(crate) fn replace_in(self, merged: &mut MergedTemporalHistory) {
        let HubAggregates { ids, entities, warnings } = self;
        merged.entities.retain(|entity| entity.id().is_none_or(|id| !ids.contains(id)));
        merged.entities.extend(entities);
        merged.warnings.extend(warnings);
    }
}

/// Aggregate here the entities the gathering of `shape` selects (every entity
/// a source answered, or only those held in more than one part), taking them
/// out of `local` and closing the remaining local aggregates and the new ones
/// to one whole-range period (clause 4.5.19.1).
///
/// Nothing is aggregated here when the shape renders as answered or over the
/// split filters, or when no source answered.
///
/// # Errors
///
/// The failures of the second round's forwarding, and the classified failure
/// of an aggregation that exceeds its bound.
pub(crate) async fn aggregate_at_hub(
    service: &TemporalService,
    sources: TemporalForwarding<'_>,
    request: GatheringRequest<'_>,
    shape: ContributionShape,
    local: &mut Vec<TemporalHistory>,
    forwarded: Option<&ForwardedHistory>,
) -> Result<HubAggregates, ProblemResponse> {
    let (ContributionShape::HubAggregated(gathering), Some(history)) = (shape, forwarded.filter(|history| history.holds_remote_entities())) else {
        return Ok(HubAggregates::default());
    };
    let (parts, mut warnings) = match gathering {
        AggregationGathering::EveryInstance => (
            // clone: the sources' answers stay with the history their warnings and contributions are read from
            parts_by_entity(instance_documents(&history.sources).cloned()),
            Vec::new(),
        ),
        AggregationGathering::OverlapsAgain => regather(sources, request, overlapping_entities(local, &history.sources)).await?,
    };
    if parts.is_empty() {
        return Ok(HubAggregates {
            warnings,
            ..HubAggregates::default()
        });
    }
    let parts = with_local_parts(local, parts);
    let ids: HashSet<Identifier> = parts.iter().map(|part| part.entity_id.clone()).collect(); // clone: the set outlives the parts the rendering consumes
    let GatheringRequest { target, query } = request;
    let reading = QueryReading {
        context: query.context,
        language_targeting: query.language_targeting,
    };
    let (mut entities, rendering_warnings) = render_merged_instances(service, target, query.params, reading, parts, query.instance).await?;
    warnings.extend(rendering_warnings);
    local.retain(|entity| entity.id().is_none_or(|id| !ids.contains(id)));
    close_hub_aggregates(query.params, local, &mut entities);
    Ok(HubAggregates { ids, entities, warnings })
}

/// Render each entity of `parts`, against `target` under `params`, with the aggregates of the instances the sources
/// answered for it, together with its local instances when its local part is included.
///
/// # Errors
///
/// The classified failure of compiling the query or of an aggregation that
/// exceeds its bound.
pub(crate) async fn render_merged_instances(
    service: &TemporalService,
    target: &ContextTarget,
    params: &TemporalQueryParameters,
    reading: QueryReading<'_>,
    parts: Vec<EntityParts>,
    instance: &Uri,
) -> Result<(Vec<TemporalHistory>, Vec<NgsiLdWarning>), ProblemResponse> {
    let rendering = service
        .render_merged_aggregations(target, params, reading, parts)
        .await
        .with_instance(instance)?;
    let warnings = rendering
        .warnings
        .into_iter()
        .map(|warning| NgsiLdWarning::new(WarnCode::RevalidationFailed, warning))
        .collect();
    Ok((rendering.entities, warnings))
}

/// Each entity of `parts` with the documents the sources answered for it, its local part included only when the
/// local read returned the entity.
///
/// The local read holds exactly the local parts that belong to the merged entities. Without split entities it applied
/// the value, geo and scope filters before the merge, the sets S2 to S4 of clause 5.7.4.4, so an entity it left out
/// failed them locally and its local instances are no part of the entity merged into S4. With split entities this
/// broker aggregates only a query carrying none of those filters, so the read left out no local part holding an
/// instance of the window. The read is never cut to a page: a distributed query reads past its result budget only to
/// be refused.
fn with_local_parts(local: &[TemporalHistory], parts: Vec<(Identifier, Vec<TemporalDocument>)>) -> Vec<EntityParts> {
    let read: HashSet<&Identifier> = local.iter().filter_map(TemporalHistory::id).collect();
    parts
        .into_iter()
        .map(|(entity_id, answered)| EntityParts {
            local: if read.contains(&entity_id) {
                LocalPart::Included
            } else {
                LocalPart::Excluded
            },
            entity_id,
            answered,
        })
        .collect()
}

/// The entities more than one part answered: this broker's local read and
/// each source's answer count as one part each, however many pages they took.
fn overlapping_entities<'a>(local: &'a [TemporalHistory], sources: &'a [ForwardedTemporalEntities]) -> Vec<Identifier> {
    let local_parts = local.iter().filter_map(TemporalHistory::id).map(|entity_id| (entity_id, 0));
    let source_parts = sources.iter().enumerate().flat_map(|(position, source)| {
        source
            .entities
            .iter()
            .filter_map(TemporalHistory::id)
            .map(move |entity_id| (entity_id, position + 1))
    });
    let mut parts: Vec<(&Identifier, usize)> = local_parts.chain(source_parts).collect();
    parts.sort_unstable_by(|(left, left_part), (right, right_part)| left.as_str().cmp(right.as_str()).then(left_part.cmp(right_part)));
    parts.dedup();
    let parts = parts;
    parts
        .chunk_by(|(left, _), (right, _)| left == right)
        .filter(|holders| holders.len() > 1)
        .filter_map(|holders| holders.first().map(|(entity_id, _)| (*entity_id).clone())) // clone: the second round owns the identifiers it asks for
        .collect()
}

/// Ask the sources again for every instance of the window of `overlapping`,
/// and the clause 6.3.17 warnings of that round.
///
/// # Errors
///
/// The failures of [`forward_temporal_history`].
async fn regather(
    sources: TemporalForwarding<'_>,
    request: GatheringRequest<'_>,
    overlapping: Vec<Identifier>,
) -> Result<(Vec<(Identifier, Vec<TemporalDocument>)>, Vec<NgsiLdWarning>), ProblemResponse> {
    if overlapping.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let GatheringRequest { target, query } = request;
    // clone: the second round asks the client's query of the overlapping entities alone
    let mut params = query.params.clone();
    params.id = Some(overlapping);
    let params = params;
    let sources = TemporalForwarding {
        reach: HistoryReach::Complete,
        ..sources
    };
    let answered = forward_temporal_history(sources, QueryScope::Distributed, target, &params, query.context, query.instance).await?;
    let Some(answered) = answered else {
        return Ok((Vec::new(), Vec::new()));
    };
    let warnings = collect_temporal_warnings(&answered.sources);
    let documents = answered
        .sources
        .into_iter()
        .flat_map(|source| source.entities)
        .filter_map(|entity| match entity {
            TemporalHistory::Instances(document) => Some(document),
            TemporalHistory::Aggregates(_) => None,
        });
    Ok((parts_by_entity(documents), warnings))
}

/// The instance documents of `documents` gathered by the entity they belong
/// to, ordered by identifier, each entity's documents in the order they came.
fn parts_by_entity(documents: impl Iterator<Item = TemporalDocument>) -> Vec<(Identifier, Vec<TemporalDocument>)> {
    // clone: each part is keyed by the identifier its documents name
    let mut named: Vec<(Identifier, TemporalDocument)> = documents
        .filter_map(|document| document.id().cloned().map(|entity_id| (entity_id, document)))
        .collect();
    named.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));
    let named = named;
    let mut parts: Vec<(Identifier, Vec<TemporalDocument>)> = Vec::new();
    for (entity_id, document) in named {
        match parts.last_mut() {
            Some((held, documents)) if *held == entity_id => documents.push(document),
            Some(_) | None => parts.push((entity_id, vec![document])),
        }
    }
    parts
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
