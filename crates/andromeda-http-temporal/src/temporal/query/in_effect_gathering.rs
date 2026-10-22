//! Gathering the instances a split temporal query's filters read in effect
//! when its window opens, from every part of the entities it judges (clauses
//! 4.11 and 5.7.4.4).
//!
//! The window query a source answers holds the instances inside the window
//! only, and the local read decides its own part, so the instance each stream
//! held when the window opens, the latest across every part, is gathered
//! apart: each source is probed for the latest instance and the latest
//! deletion of every filter attribute before the start, this broker reads its
//! own, and every dataset a source answering per Attribute may have hidden is
//! probed for on its own. An entity a source answered a probe for is judged
//! over every part even when no source answered it for the window.

use crate::temporal::{
    ProblemResponse,
    ResultInstanceExt,
    query::{
        forwarding::{ForwardedHistory, InEffectRequest, TemporalForwarding, forward_in_effect_probes},
        split_contribution::carries_split_filters,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{
    config::SplitEntities,
    in_effect_probe::{InEffectProbe, InEffectReading},
    outcome::ForwardedTemporalEntities,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_temporal::{in_effect_follow_up::follow_up_datasets, local_in_effect::LocalInEffect, service::TemporalService};
use std::{collections::HashMap, slice};

/// The probes asked across every dataset.
const ACROSS_DATASETS: [InEffectProbe<'static>; 2] = [
    InEffectProbe {
        reading: InEffectReading::Placed,
        dataset: None,
    },
    InEffectProbe {
        reading: InEffectReading::Deletion,
        dataset: None,
    },
];

/// What every part holds in effect when a split query's window opens.
pub(crate) struct InEffectGathering {
    /// What each source answered each probe with.
    pub(crate) sources: Vec<ForwardedTemporalEntities>,
    /// What this broker holds in effect of every entity judged.
    pub(crate) local: LocalInEffect,
}

impl InEffectGathering {
    /// Every part a source answered a probe with.
    pub(crate) fn answered_documents(&self) -> impl Iterator<Item = &TemporalDocument> {
        instance_documents(&self.sources)
    }
}

/// Every part `sources` answered with its instances; an aggregate holds none.
pub(crate) fn instance_documents(sources: &[ForwardedTemporalEntities]) -> impl Iterator<Item = &TemporalDocument> {
    sources.iter().flat_map(|source| source.entities.iter()).filter_map(|answer| match answer {
        TemporalHistory::Instances(document) => Some(document),
        TemporalHistory::Aggregates(_) => None,
    })
}

/// What gathering the in-effect parts reads of the request.
#[derive(Clone, Copy)]
pub(crate) struct GatheringRequest<'r> {
    /// The branch the query reads.
    pub(crate) target: &'r ContextTarget,
    /// The query, its context and the URI a failure names.
    pub(crate) query: InEffectRequest<'r>,
    /// The engine the context source filter is expanded with.
    pub(crate) engine: &'r JsonLdEngine,
}

/// Gather what every part holds in effect when the window opens, of every
/// entity a source answered the window or a probe with; `None` unless the
/// query is split, distributed, carries a value, geo or scope filter, and its
/// window has an instance in effect before it.
///
/// # Errors
///
/// The failures of [`forward_in_effect_probes`], and the classified failure of
/// reading this broker's part.
pub(crate) async fn gather_in_effect(
    sources: TemporalForwarding<'_>,
    service: &TemporalService,
    request: GatheringRequest<'_>,
    filtering: SplitEntities,
    forwarded: Option<&ForwardedHistory>,
) -> Result<Option<InEffectGathering>, ProblemResponse> {
    let GatheringRequest { target, query, engine } = request;
    let opens_with_an_instance_in_effect = query.params.window().ok().and_then(|window| window.in_effect_start()).is_some();
    let (Some(forwarded), SplitEntities::Split, true, true) = (forwarded, filtering, carries_split_filters(query.params), opens_with_an_instance_in_effect)
    else {
        return Ok(None);
    };
    let mut probed = forward_in_effect_probes(sources, engine, target, query, &ACROSS_DATASETS).await?;
    let window_parts: Vec<&TemporalDocument> = instance_documents(&forwarded.sources).collect();
    let mut entity_ids: Vec<Identifier> = window_parts
        .iter()
        .copied()
        .chain(instance_documents(&probed))
        .filter_map(TemporalDocument::id)
        .cloned() // clone: the local read is asked about the identifiers the parts keep
        .collect();
    entity_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    entity_ids.dedup();
    let local = service
        .read_local_in_effect(target, query.params, query.context, &entity_ids)
        .await
        .with_instance(query.instance)?;
    let datasets = follow_up_datasets(&by_source(&probed), local.parts().chain(window_parts));
    let follow_ups: Vec<InEffectProbe<'_>> = datasets
        .iter()
        .flat_map(|dataset| {
            [InEffectReading::Placed, InEffectReading::Deletion].map(|reading| InEffectProbe {
                reading,
                dataset: Some(dataset),
            })
        })
        .collect();
    if !follow_ups.is_empty() {
        probed.extend(forward_in_effect_probes(sources, engine, target, query, &follow_ups).await?);
    }
    Ok(Some(InEffectGathering { sources: probed, local }))
}

/// The parts `probed` answered, one list per source.
fn by_source(probed: &[ForwardedTemporalEntities]) -> Vec<Vec<&TemporalDocument>> {
    let mut sources: HashMap<Option<&Identifier>, Vec<&TemporalDocument>> = HashMap::new();
    for answer in probed {
        sources
            .entry(answer.registration_id.as_ref())
            .or_default()
            .extend(instance_documents(slice::from_ref(answer)));
    }
    sources.into_values().collect()
}
