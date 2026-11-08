//! The instances the entities a split temporal query's hops reach hold in
//! effect when its window opens (clauses 4.11 and 5.7.4.4, under
//! `temporal_linked_retrieval`).
//!
//! Clause 4.11 counts among the instances valid within a window the one each
//! stream held when it opens, and a hop target may be held by this broker, by
//! any context source covering it, or by several, so that instance is the
//! latest across all of them, as it is for a root entity, and a deletion at
//! the winning placement leaves nothing in effect. Every holder is asked with
//! standard parameters alone, for the attributes the hops read of the target:
//! this broker reads the latest instance of each of its streams, and every
//! covering source is asked for the latest instance before the start on the
//! window's Temporal Property and for the latest deletion on `deletedAt`. A
//! source applying `lastN` per Attribute answers one dataset only, so every
//! dataset another holder shows and a source left out is asked about on its
//! own, and a source that answered a deletion is asked again for the older
//! deletions of that stream until they can no longer pass the instance in
//! effect. A holder that fails leaves its part out and is reported.

use crate::{
    deletion_follow_up::{SourcedDocuments, follow_earlier_deletions},
    error::TemporalError,
    hop_targets::TargetRead,
    in_effect_follow_up::follow_up_datasets,
    local_in_effect::part_document,
    port::in_effect_reads::{InEffectReads, ScopeRead},
    remote_linked::{HeldPart, RemoteTemporalLinkedSource},
    service::TemporalService,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    temporal::{earlier_deletions::EarlierDeletions, window::TemporalWindow},
};
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use chrono::{DateTime, Utc};
use std::{collections::HashMap, convert::Infallible};

/// What the targets of one hop depth hold in effect when the window opens.
#[derive(Debug, Default)]
pub(crate) struct LevelInEffect {
    /// Every holder's answers, unmerged: the latest per stream is decided
    /// across them when the hop is judged.
    pub(crate) parts: Vec<TemporalDocument>,
    /// The holders that could not be consulted or whose answer was unusable.
    pub(crate) incomplete: Vec<LinkedResolutionFailure>,
}

impl TemporalService {
    /// Read what `ids` hold in effect when the window of `read` opens, from
    /// this broker for those of them `catalogued` records and from every
    /// holder outside it; `window_parts` holds the targets read over the
    /// window, whose datasets count among those a source is asked about.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when this broker's history cannot be read or
    /// a stored row no longer reads as the instance it recorded.
    pub(crate) async fn read_level_in_effect(
        &self,
        read: &TargetRead<'_>,
        ids: &[Identifier],
        catalogued: &HashMap<Identifier, Vec<EntityTypeName>>,
        window_parts: &[TemporalDocument],
    ) -> Result<LevelInEffect, TemporalError> {
        let mut level = LevelInEffect::default();
        let Some(start) = read.compiled.window.in_effect_start() else {
            return Ok(level);
        };
        if read.in_effect.is_empty() {
            return Ok(level);
        }
        let recorded: Vec<Identifier> = ids.iter().filter(|id| catalogued.contains_key(*id)).cloned().collect();
        if !recorded.is_empty() {
            let reads = InEffectReads {
                attributes: read.in_effect,
                scope: ScopeRead::Unread,
            };
            for (id, rows) in self.reader.in_effect_instances(read.scope, read.compiled, &recorded, reads).await? {
                let types = catalogued.get(&id).map_or(&[][..], Vec::as_slice);
                level.parts.push(part_document(&id, types, rows)?);
            }
        }
        let Some(remote) = self.remote_temporal_linked.as_deref().filter(|_| read.target.permits_federation()) else {
            return Ok(level);
        };
        for id in ids {
            let holders = Holders {
                remote,
                id,
                window: read.compiled.window,
                start,
                attributes: read.in_effect,
                context: read.context,
            };
            let known: Vec<&TemporalDocument> = level.parts.iter().chain(window_parts).filter(|part| part.id() == Some(id)).collect();
            let (answers, incomplete) = holders.gather(&known).await;
            level.parts.extend(answers.into_iter().filter_map(|answer| answer.history.into_instances()));
            level.incomplete.extend(incomplete);
        }
        Ok(level)
    }
}

/// The holders of one hop target outside this broker, asked about the
/// instance in effect when the window opens.
struct Holders<'h> {
    /// The remainder of the system.
    remote: &'h dyn RemoteTemporalLinkedSource,
    /// The target.
    id: &'h Identifier,
    /// The window of the query.
    window: TemporalWindow,
    /// The instant it opens at.
    start: DateTime<Utc>,
    /// The attributes the hops read of the target.
    attributes: &'h [&'h AttributeName],
    /// The request context.
    context: &'h ContextSpec,
}

impl Holders<'_> {
    /// Every answer the holders gave, beside the failures of those that did
    /// not answer; `known` holds the parts already read of the target.
    async fn gather(&self, known: &[&TemporalDocument]) -> (Vec<HeldPart>, Vec<LinkedResolutionFailure>) {
        let mut incomplete = Vec::new();
        let mut placed = self.latest(self.window.timeproperty, None, &mut incomplete).await;
        let mut deletions = self.latest(TimeProp::Deleted, None, &mut incomplete).await;
        let datasets = follow_up_datasets(&by_holder(placed.iter().chain(&deletions)), known.iter().copied());
        for dataset in &datasets {
            placed.extend(self.latest(self.window.timeproperty, Some(dataset), &mut incomplete).await);
            deletions.extend(self.latest(TimeProp::Deleted, Some(dataset), &mut incomplete).await);
        }
        let held: Vec<&TemporalDocument> = known.iter().copied().chain(placed.iter().filter_map(|part| part.history.instances())).collect();
        let followed = follow_earlier_deletions(self.window, &held, deletions, held_documents, async |request| {
            Ok::<_, Infallible>(self.earlier(request, &mut incomplete).await)
        })
        .await;
        let Ok(deletions) = followed;
        placed.extend(deletions);
        (placed, incomplete)
    }

    /// What every holder answers for the latest instance of the attributes
    /// placed on `timeproperty` before the start, in `dataset` alone when one
    /// is named.
    async fn latest(&self, timeproperty: TimeProp, dataset: Option<&DatasetSelector>, incomplete: &mut Vec<LinkedResolutionFailure>) -> Vec<HeldPart> {
        let attributes = self.attributes.iter().map(|name| (*name).clone()).collect();
        let params = TemporalQueryParameters::latest_before(self.start, timeproperty, Some(attributes), dataset.cloned());
        self.ask(&params, None, incomplete).await
    }

    /// What the one source `request` names answers for the deletion of its
    /// stream before the `deletedAt` it carries.
    async fn earlier(&self, request: &EarlierDeletions, incomplete: &mut Vec<LinkedResolutionFailure>) -> Vec<HeldPart> {
        let params = TemporalQueryParameters::latest_before(
            request.before,
            TimeProp::Deleted,
            Some(vec![request.attribute.clone()]),
            Some(request.dataset.clone()),
        );
        self.ask(&params, Some(&request.source), incomplete).await
    }

    /// What the holders, or the one registration `only` names, answer for
    /// `params`; a holder that fails is reported in `incomplete`.
    async fn ask(&self, params: &TemporalQueryParameters, only: Option<&Identifier>, incomplete: &mut Vec<LinkedResolutionFailure>) -> Vec<HeldPart> {
        match self.remote.fetch_held_parts(self.id, params, only, self.context).await {
            Ok(resolution) => {
                for failure in &resolution.failures {
                    tracing::warn!("Hop target {} was not fully read in effect: {} ({})", failure.id, failure.detail, failure.kind);
                }
                incomplete.extend(resolution.failures);
                resolution.entities
            }
            Err(error) => {
                tracing::warn!("Failed to read what hop target {} holds in effect outside this broker: {}", self.id, error);
                incomplete.push(LinkedResolutionFailure::unreachable(self.id, &error));
                Vec::new()
            }
        }
    }
}

/// The parts `answers` hold, one list per holder.
fn by_holder<'a>(answers: impl IntoIterator<Item = &'a HeldPart>) -> Vec<Vec<&'a TemporalDocument>> {
    let mut holders: HashMap<Option<&Identifier>, Vec<&TemporalDocument>> = HashMap::new();
    for answer in answers {
        holders.entry(answer.registration.as_ref()).or_default().extend(answer.history.instances());
    }
    holders.into_values().collect()
}

/// The holder `part` came from and the instances it holds.
fn held_documents(part: &HeldPart) -> SourcedDocuments<'_> {
    SourcedDocuments {
        source: part.registration.as_ref(),
        documents: part.history.instances().into_iter().collect(),
    }
}
