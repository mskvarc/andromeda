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
//! probed for on its own. The latest deletion is the one with the latest
//! `deletedAt`, which an undated deletion placed after the instance in effect
//! can fall before, so a source that answered a deletion is asked again for
//! the older deletions of that stream until they can no longer pass the
//! instance in effect. An entity a source answered a probe for is judged over
//! every part even when no source answered it for the window.

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
    in_effect_probe::{InEffectProbe, InEffectReading},
    outcome::ForwardedTemporalEntities,
    split_entities::SplitEntities,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, filter::language_targeting::LanguageTargeting, temporal::window::TemporalWindow};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_temporal::{
    deletion_follow_up::{DeletionAnswer, earlier_deletions},
    in_effect_follow_up::follow_up_datasets,
    local_in_effect::LocalInEffect,
    query_reading::QueryReading,
    service::TemporalService,
};
use axum::http::Uri;
use chrono::{DateTime, Utc};
use std::{collections::HashMap, iter, slice};

/// The probe for the latest instance, asked across every dataset.
const PLACED_ACROSS_DATASETS: InEffectProbe<'static> = InEffectProbe {
    reading: InEffectReading::Placed,
    dataset: None,
};

/// The probe for the latest deletion, asked across every dataset.
const DELETION_ACROSS_DATASETS: InEffectProbe<'static> = InEffectProbe {
    reading: InEffectReading::Deletion,
    dataset: None,
};

/// What gathering has read of the parts of the entities judged, apart from
/// the deletions it is following up.
#[derive(Clone, Copy)]
struct HeldParts<'a> {
    /// What this broker holds in effect.
    local: &'a LocalInEffect,
    /// The parts the sources answered the window query with.
    window_parts: &'a [&'a TemporalDocument],
    /// What the sources answered the instance probes with.
    probed: &'a [ForwardedTemporalEntities],
}

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
    target: &'r ContextTarget,
    /// The query, its context and the URI a failure names.
    query: InEffectRequest<'r>,
}

impl<'r> GatheringRequest<'r> {
    /// The gathering of `params` against `target`, under `context`, its
    /// target languages read under `language_targeting` and a failure naming
    /// `instance`.
    pub(crate) const fn new(
        target: &'r ContextTarget,
        params: &'r TemporalQueryParameters,
        context: &'r ContextSpec,
        language_targeting: LanguageTargeting,
        instance: &'r Uri,
    ) -> GatheringRequest<'r> {
        GatheringRequest {
            target,
            query: InEffectRequest {
                params,
                context,
                language_targeting,
                instance,
            },
        }
    }
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
    let GatheringRequest { target, query } = request;
    let window = query
        .params
        .window()
        .ok()
        .and_then(|window| window.in_effect_start().map(|start| (window, start)));
    let (Some(forwarded), SplitEntities::Split, true, Some((window, start))) = (forwarded, filtering, carries_split_filters(query.params), window) else {
        return Ok(None);
    };
    let mut probed = forward_in_effect_probes(sources, target, query, &[PLACED_ACROSS_DATASETS]).await?;
    let mut deletions = forward_in_effect_probes(sources, target, query, &[DELETION_ACROSS_DATASETS]).await?;
    let window_parts: Vec<&TemporalDocument> = instance_documents(&forwarded.sources).collect();
    let mut entity_ids: Vec<Identifier> = window_parts
        .iter()
        .copied()
        .chain(instance_documents(&probed))
        .chain(instance_documents(&deletions))
        .filter_map(TemporalDocument::id)
        .cloned() // clone: the local read is asked about the identifiers the parts keep
        .collect();
    entity_ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
    entity_ids.dedup();
    let local = service
        .read_local_in_effect(
            target,
            query.params,
            QueryReading {
                context: query.context,
                language_targeting: query.language_targeting,
            },
            &entity_ids,
        )
        .await
        .with_instance(query.instance)?;
    let datasets = follow_up_datasets(&by_source(probed.iter().chain(&deletions)), local.parts().chain(window_parts.iter().copied()));
    if !datasets.is_empty() {
        probed.extend(forward_in_effect_probes(sources, target, query, &dataset_probes(&datasets, InEffectReading::Placed)).await?);
        deletions.extend(forward_in_effect_probes(sources, target, query, &dataset_probes(&datasets, InEffectReading::Deletion)).await?);
    }
    let held = HeldParts {
        local: &local,
        window_parts: &window_parts,
        probed: &probed,
    };
    let deletions = follow_up_deletions(sources, target, query, (window, start), held, deletions).await?;
    probed.extend(deletions);
    Ok(Some(InEffectGathering { sources: probed, local }))
}

/// One probe of `reading` per dataset of `datasets`.
fn dataset_probes<'a>(datasets: &'a [DatasetSelector], reading: InEffectReading<'static>) -> Vec<InEffectProbe<'a>> {
    datasets
        .iter()
        .map(|dataset| InEffectProbe {
            reading,
            dataset: Some(dataset),
        })
        .collect()
}

/// Ask each source that answered a deletion again, for the older deletions of
/// the stream it answered (clauses 4.5.7 and 4.11), until the streams are
/// followed to the instance in effect; `deletions` and the answers to the
/// repeated requests.
///
/// `window` is the window of the query and the instant it opens at, the bound
/// the first probes carried.
///
/// # Errors
///
/// The failures of [`forward_in_effect_probes`].
async fn follow_up_deletions(
    sources: TemporalForwarding<'_>,
    target: &ContextTarget,
    query: InEffectRequest<'_>,
    (window, start): (TemporalWindow, DateTime<Utc>),
    held: HeldParts<'_>,
    mut deletions: Vec<ForwardedTemporalEntities>,
) -> Result<Vec<ForwardedTemporalEntities>, ProblemResponse> {
    let mut bounds = vec![start; deletions.len()];
    let mut latest_round = 0;
    loop {
        let requests = {
            let answers: Vec<DeletionAnswer<'_>> = deletions[latest_round..]
                .iter()
                .zip(&bounds[latest_round..])
                .filter_map(|(answer, bound)| {
                    Some(DeletionAnswer {
                        source: answer.registration_id.as_ref()?,
                        bound: *bound,
                        documents: instance_documents(slice::from_ref(answer)).collect(),
                    })
                })
                .collect();
            let known = held
                .local
                .parts()
                .chain(held.window_parts.iter().copied())
                .chain(instance_documents(held.probed))
                .chain(instance_documents(&deletions));
            earlier_deletions(&answers, known, window)
        };
        if requests.is_empty() {
            return Ok(deletions);
        }
        latest_round = deletions.len();
        for request in &requests {
            let probe = InEffectProbe {
                reading: InEffectReading::EarlierDeletion(request),
                dataset: None,
            };
            let answers = forward_in_effect_probes(sources, target, query, &[probe]).await?;
            bounds.extend(iter::repeat_n(request.before, answers.len()));
            deletions.extend(answers);
        }
    }
}

/// The parts `probed` answered, one list per source.
fn by_source<'a>(probed: impl IntoIterator<Item = &'a ForwardedTemporalEntities>) -> Vec<Vec<&'a TemporalDocument>> {
    let mut sources: HashMap<Option<&Identifier>, Vec<&TemporalDocument>> = HashMap::new();
    for answer in probed {
        sources
            .entry(answer.registration_id.as_ref())
            .or_default()
            .extend(instance_documents(slice::from_ref(answer)));
    }
    sources.into_values().collect()
}
