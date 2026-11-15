//! Gathering the Scope a split temporal query's scope filter reads in force
//! when its window opens, from every part of the entities it judges (clauses
//! 4.18 and 5.7.4.4).
//!
//! The window query a source answers holds the instances inside the window
//! only, and the local read decides its own part, so the Scope in force when
//! the window opens, the latest across every part, is gathered apart: each
//! source is probed for the latest instance and the latest deletion before the
//! start, and this broker reads its own. The value and geo filters read the
//! instances inside the window alone (clause 5.7.4.4 steps S2 and S3), so only
//! a query carrying a scope filter gathers. An entity a source answered a probe
//! for is judged over every part even when no source answered it for the
//! window.

use crate::temporal::{
    ProblemResponse,
    ResultInstanceExt,
    query::forwarding::{ForwardedHistory, InEffectRequest, TemporalForwarding, forward_in_effect_probes},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{in_effect_probe::InEffectReading, outcome::ForwardedTemporalEntities, split_entities::SplitEntities};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;
use andromeda_ngsi_ld_request::query::temporal::admitted::AdmittedTemporalRequest;
use andromeda_temporal::{local_in_effect::LocalInEffect, query_reading::QueryReading, service::TemporalService};
use axum::http::Uri;

/// The Scope every part holds in force when a split query's window opens.
pub(crate) struct InEffectGathering {
    /// What each source answered each probe with.
    pub(crate) sources: Vec<ForwardedTemporalEntities>,
    /// The Scope this broker holds in force for every entity judged.
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
    sources.iter().flat_map(|source| source.entities.iter()).filter_map(TemporalHistory::instances)
}

/// What gathering parts of the queried entities reads of the request: the
/// Scope parts of a split query, and the instances of the entities an
/// aggregated query aggregates here.
#[derive(Clone, Copy)]
pub(crate) struct GatheringRequest<'r> {
    /// The branch the query reads.
    pub(crate) target: &'r ContextTarget,
    /// The query, its context and the URI a failure names.
    pub(crate) query: InEffectRequest<'r>,
}

impl<'r> GatheringRequest<'r> {
    /// The gathering of `params` against `target`, under `context`, its
    /// target languages read under `language_targeting` and a failure naming
    /// `instance`.
    pub(crate) const fn new(
        target: &'r ContextTarget,
        params: &'r AdmittedTemporalRequest,
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

/// Gather the Scope every part holds in force when the window opens, of every
/// entity a source answered the window or a probe with; `None` unless the
/// query is split, distributed, carries a scope filter, and its window has an
/// instance in effect before it.
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
    let has_start = query.params.window().is_ok_and(|window| window.in_effect_start().is_some());
    let (Some(forwarded), SplitEntities::Split, true, true) = (forwarded, filtering, query.params.scope_q.is_some(), has_start) else {
        return Ok(None);
    };
    let probed = forward_in_effect_probes(sources, target, query, &[InEffectReading::Placed, InEffectReading::Deletion]).await?;
    let mut entity_ids: Vec<Identifier> = instance_documents(&forwarded.sources)
        .chain(instance_documents(&probed))
        .filter_map(TemporalDocument::id)
        .cloned()
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
    Ok(Some(InEffectGathering { sources: probed, local }))
}
