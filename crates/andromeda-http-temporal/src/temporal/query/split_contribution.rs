//! The entities a Context Source contributed a part of to a split temporal
//! query, and the filters decided over their gathered parts (clause 5.7.4.4).
//!
//! Under `splitEntities` the value, geo and scope filters are applied "after
//! the Entity information from different Context Sources and local
//! information, if there is any, has been aggregated". An entity no source
//! answered with has no part outside this broker, so the local query keeps
//! deciding it in the store; only the entities a source answered the window
//! or an in-effect probe with are exempt locally and judged here, over every
//! part, once the sources have answered. An entity holding nothing inside the
//! window in any part is never answered: the judgement only drops entities.

use crate::temporal::{
    ProblemResponse,
    ResultInstanceExt,
    TemporalQueryParameters,
    collect_temporal_warnings,
    query::{
        forwarding::{ForwardedHistory, HistoryReach},
        hub_aggregation::render_merged_instances,
        in_effect_gathering::{InEffectGathering, instance_documents},
        merged_history::{MergedTemporalHistory, RemoteParts, merge_forwarded_history},
        whole_range_join::close_hub_aggregates,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{aggregation_gathering::AggregationGathering, split_entities::SplitEntities};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::linked_resolution_warning::with_linked_resolution_warnings;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::{
    filter::language_targeting::LanguageTargeting,
    temporal::contributed_entities::{ContributedEntities, ContributedRendering},
};
use andromeda_temporal::{
    error::TemporalError,
    local_in_effect::LocalInEffect,
    merged_parts::{EntityParts, LocalPart},
    query_reading::QueryReading,
    service::TemporalService,
    split_settlement::{AnsweredParts, SplitSettlement},
};
use axum::http::Uri;

/// How a split query gathers and renders the entities a source contributed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContributionShape {
    /// The sources answer in the representation the client asked for, and
    /// their answers are merged with the local one.
    AsAnswered,
    /// The query asks for an aggregated representation and carries a filter:
    /// clause 5.7.4.4 renders a contributed entity with the aggregates of its
    /// merged instances, so the sources answer with every instance of the
    /// window and the entity is aggregated here.
    MergedInstances,
    /// The query asks for an aggregated representation and decides no filter
    /// after the merge: an entity held in more than one part is aggregated
    /// here over the instances of every part (clause 5.7.4.4), found as the
    /// deployment's gathering says.
    HubAggregated(AggregationGathering),
}

impl ContributionShape {
    /// The shape `params` takes under `filtering`, an aggregated one gathering
    /// the instances of entities held in several parts as `gathering` says.
    pub(crate) fn of(filtering: SplitEntities, params: &TemporalQueryParameters, gathering: AggregationGathering) -> ContributionShape {
        let aggregated = params.parse_format().is_ok_and(|format| format.is_aggregated());
        match filtering {
            SplitEntities::Split if aggregated && carries_split_filters(params) => ContributionShape::MergedInstances,
            SplitEntities::Split | SplitEntities::Complete if aggregated => ContributionShape::HubAggregated(gathering),
            SplitEntities::Split | SplitEntities::Complete => ContributionShape::AsAnswered,
        }
    }

    /// How much of each source's history is read.
    pub(crate) const fn reach(self) -> HistoryReach {
        match self {
            ContributionShape::AsAnswered | ContributionShape::HubAggregated(AggregationGathering::OverlapsAgain) => HistoryReach::AsAnswered,
            ContributionShape::MergedInstances | ContributionShape::HubAggregated(AggregationGathering::EveryInstance) => HistoryReach::Complete,
        }
    }

    /// How the local read renders a contributed entity.
    pub(crate) const fn rendering(self) -> ContributedRendering {
        match self {
            ContributionShape::AsAnswered | ContributionShape::HubAggregated(AggregationGathering::EveryInstance | AggregationGathering::OverlapsAgain) => {
                ContributedRendering::Local
            }
            ContributionShape::MergedInstances => ContributedRendering::Merged,
        }
    }

    /// Whether the sources' entities join the local ones.
    pub(crate) const fn remote_parts(self) -> RemoteParts {
        match self {
            ContributionShape::AsAnswered | ContributionShape::HubAggregated(AggregationGathering::OverlapsAgain) => RemoteParts::Merged,
            ContributionShape::MergedInstances | ContributionShape::HubAggregated(AggregationGathering::EveryInstance) => RemoteParts::RenderedApart,
        }
    }
}

/// Render every contributed entity the filters kept with the aggregates of
/// its merged instances, when the shape asks for it; `None` otherwise.
///
/// # Errors
///
/// The classified failure of compiling the query or of an aggregation that
/// exceeds its bound.
async fn render_contributions(
    service: &TemporalService,
    request: SettledRequest<'_>,
    shape: ContributionShape,
    settlement: Option<&SplitSettlement>,
    instance: &Uri,
) -> Result<Option<(Vec<TemporalHistory>, Vec<NgsiLdWarning>)>, ProblemResponse> {
    let (ContributionShape::MergedInstances, Some(history), Some(settlement)) = (shape, request.forwarded, settlement) else {
        return Ok(None);
    };
    // Every part is merged before a split query's filters are decided, so the local part of a retained entity is in it.
    let mut parts: Vec<EntityParts> = settlement
        .retained
        .iter()
        .map(|entity_id| EntityParts {
            entity_id: entity_id.clone(), // clone: each part list owns the identifier it is rendered under
            local: LocalPart::Included,
            answered: Vec::new(),
        })
        .collect();
    for answer in instance_documents(&history.sources) {
        let Some(entity_id) = answer.id() else {
            continue;
        };
        if let Some(part) = parts.iter_mut().find(|part| part.entity_id == *entity_id) {
            // clone: the sources' answers stay with the history their warnings and contributions are read from
            part.answered.push(answer.clone());
        }
    }
    let rendered = render_merged_instances(service, request.target, request.params, request.reading(), parts, instance).await?;
    Ok(Some(rendered))
}

/// What settling and rendering the contributed entities reads of the request.
#[derive(Clone, Copy)]
struct SettledRequest<'r> {
    /// The branch the query reads.
    target: &'r ContextTarget,
    /// The query.
    params: &'r TemporalQueryParameters,
    /// The request context.
    context: &'r ContextSpec,
    /// What the query's target languages may be and how they select
    /// languageMap keys.
    language_targeting: LanguageTargeting,
    /// What the sources answered.
    forwarded: Option<&'r ForwardedHistory>,
    /// What every part holds in effect when the window opens.
    in_effect: Option<&'r InEffectGathering>,
}

impl SettledRequest<'_> {
    /// How the query's text is read.
    const fn reading(&self) -> QueryReading<'_> {
        QueryReading {
            context: self.context,
            language_targeting: self.language_targeting,
        }
    }
}

/// Whether the query carries any of the filters clause 5.7.4.4 applies after
/// aggregation; without one, S7 is S4 and nothing is decided.
pub(crate) const fn carries_split_filters(params: &TemporalQueryParameters) -> bool {
    params.q.is_some() || params.geo.0.is_some() || params.scope_q.is_some()
}

/// The entities a source answered a split query with, for the window or for
/// the instances in effect when it opens, which the local read exempts from
/// the value, geo and scope filters.
///
/// Empty unless the query is split, carries one of those filters, and a
/// source answered: every other query reads exactly as one that was never
/// split.
pub(crate) fn contributed_entities(
    filtering: SplitEntities,
    params: &TemporalQueryParameters,
    forwarded: Option<&ForwardedHistory>,
    in_effect: Option<&InEffectGathering>,
    rendering: ContributedRendering,
) -> ContributedEntities {
    let Some(history) = forwarded else {
        return ContributedEntities::default();
    };
    match filtering {
        SplitEntities::Split if carries_split_filters(params) => {
            let probed = in_effect
                .into_iter()
                .flat_map(|gathering| gathering.sources.iter())
                .flat_map(|source| source.entities.iter());
            // clone: the set owns the identifiers it exempts
            ContributedEntities::new(answered_parts(history).chain(probed).filter_map(|answer| answer.id().cloned()), rendering)
        }
        SplitEntities::Split | SplitEntities::Complete => ContributedEntities::default(),
    }
}

/// Every entity part any source answered with.
fn answered_parts(history: &ForwardedHistory) -> impl Iterator<Item = &TemporalHistory> {
    history.sources.iter().flat_map(|source| source.entities.iter())
}

/// What folding the contributed entities into the merge reads of the request.
#[derive(Clone, Copy)]
pub(crate) struct ContributionMerge<'r> {
    /// The branch the query reads.
    pub(crate) target: &'r ContextTarget,
    /// The query.
    pub(crate) params: &'r TemporalQueryParameters,
    /// The request context.
    pub(crate) context: &'r ContextSpec,
    /// What the query's target languages may be and how they select
    /// languageMap keys.
    pub(crate) language_targeting: LanguageTargeting,
    /// Whether an entity's attributes may be spread over several sources.
    pub(crate) filtering: SplitEntities,
    /// How the contributed entities are gathered and rendered.
    pub(crate) shape: ContributionShape,
    /// The entities a source contributed to.
    pub(crate) contributed: &'r ContributedEntities,
}

/// Fold what the sources answered into the local entities: the filters
/// decided over every contributed entity's gathered parts, the ones they did
/// not keep dropped, and -- for an aggregated representation -- the kept ones
/// rendered over their merged instances (clause 5.7.4.4).
///
/// # Errors
///
/// The classified failure of settling the filters, of rendering a merged
/// aggregation, or the clause 4.12 refusal of a merge exceeding the budget.
pub(crate) async fn merge_with_contributions(
    service: &TemporalService,
    merge: ContributionMerge<'_>,
    local_entities: Vec<TemporalHistory>,
    forwarded: Option<ForwardedHistory>,
    in_effect: Option<InEffectGathering>,
    instance: &Uri,
) -> Result<MergedTemporalHistory, ProblemResponse> {
    let settled = SettledRequest {
        target: merge.target,
        params: merge.params,
        context: merge.context,
        language_targeting: merge.language_targeting,
        forwarded: forwarded.as_ref(),
        in_effect: in_effect.as_ref(),
    };
    let settlement = settle_contributions(service, settled, merge.contributed, instance).await?;
    let rendered = render_contributions(service, settled, merge.shape, settlement.as_ref(), instance).await?;
    let window = merge.params.window().map_err(TemporalError::from).with_instance(instance)?;
    let mut merged = merge_forwarded_history(local_entities, forwarded, merge.filtering, merge.shape.remote_parts(), window, instance)?;
    if let Some(settlement) = settlement {
        retain_settled(&mut merged.entities, merge.contributed, &settlement);
        merged.warnings.extend(with_linked_resolution_warnings(Vec::new(), &settlement.incomplete));
    }
    if let Some((mut entities, warnings)) = rendered {
        close_hub_aggregates(merge.params, &mut merged.entities, &mut entities);
        merged.entities.extend(entities);
        merged.warnings.extend(warnings);
    }
    if let Some(gathering) = &in_effect {
        merged.warnings.extend(collect_temporal_warnings(&gathering.sources));
    }
    Ok(merged)
}

/// Decide the filters over the gathered parts of the contributed entities;
/// `None` when no source contributed one.
///
/// # Errors
///
/// The classified failure of compiling the query or reading the local parts.
async fn settle_contributions(
    service: &TemporalService,
    request: SettledRequest<'_>,
    contributed: &ContributedEntities,
    instance: &Uri,
) -> Result<Option<SplitSettlement>, ProblemResponse> {
    let (Some(history), false) = (request.forwarded, contributed.is_empty()) else {
        return Ok(None);
    };
    let answered = AnsweredParts {
        in_window: instance_documents(&history.sources).collect(),
        in_effect: request.in_effect.into_iter().flat_map(InEffectGathering::answered_documents).collect(),
    };
    let nothing_local = LocalInEffect::default();
    let local = request.in_effect.map_or(&nothing_local, |gathering| &gathering.local);
    let settlement = service
        .settle_split_filters(request.target, request.params, request.reading(), &answered, local)
        .await
        .with_instance(instance)?;
    Ok(Some(settlement))
}

/// Drop every merged entity a source contributed to whose gathered parts the
/// filters did not keep.
fn retain_settled(entities: &mut Vec<TemporalHistory>, contributed: &ContributedEntities, settlement: &SplitSettlement) {
    entities.retain(|entity| {
        let Some(entity_id) = entity.id() else {
            return true;
        };
        !contributed.contains(entity_id) || settlement.retained.contains(entity_id)
    });
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        temporal::query::{
            forwarding::{ForwardedHistory, HistoryReach},
            in_effect_gathering::InEffectGathering,
            merged_history::RemoteParts,
            split_contribution::{ContributionShape, carries_split_filters, contributed_entities, retain_settled},
        },
        test_history::held_history,
    };
    use andromeda_federation::{
        aggregation_gathering::AggregationGathering,
        outcome::ForwardedTemporalEntities,
        result_budget::{ResultBudget, ResultLimits},
        split_entities::SplitEntities,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{registration::forwarding_mode::ForwardingMode, temporal::temporal_history::TemporalHistory};
    use andromeda_ngsi_ld_query::{
        scope::query::ScopeQuery,
        temporal::contributed_entities::{ContributedEntities, ContributedRendering},
    };
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
    use andromeda_temporal::{local_in_effect::LocalInEffect, split_settlement::SplitSettlement};
    use serde_json::json;
    use std::num::NonZeroU32;

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("a valid identifier")
    }

    /// The history of the entity `identifier` names, holding no attribute.
    async fn history(identifier: &str) -> TemporalHistory {
        held_history(json!({"id": identifier, "type": "Vehicle"})).await
    }

    /// Clause 5.7.4.4: "If no values filter query is provided, then S5 is
    /// equal to S4", and likewise for the geoquery and the scope query.
    #[test]
    fn only_a_value_geo_or_scope_filter_is_decided_after_aggregation() {
        assert!(!carries_split_filters(&TemporalQueryParameters::default()));
        let scoped = TemporalQueryParameters {
            scope_q: Some(ScopeQuery::parse("/Madrid").expect("a valid scopeQ")),
            ..TemporalQueryParameters::default()
        };
        assert!(carries_split_filters(&scoped));
    }

    /// A contributed entity the filters did not keep leaves the merge; an
    /// entity no source contributed to was decided by the store and stays.
    #[tokio::test]
    async fn only_contributed_entities_the_filters_rejected_are_dropped() {
        let mut entities = vec![
            history("urn:ngsi-ld:Vehicle:1").await,
            history("urn:ngsi-ld:Vehicle:2").await,
            history("urn:ngsi-ld:Vehicle:3").await,
        ];
        let contributed = ContributedEntities::new([id("urn:ngsi-ld:Vehicle:1"), id("urn:ngsi-ld:Vehicle:2")], ContributedRendering::Local);
        let settlement = SplitSettlement {
            retained: [id("urn:ngsi-ld:Vehicle:2")].into_iter().collect(),
            incomplete: Vec::new(),
        };
        retain_settled(&mut entities, &contributed, &settlement);
        pretty_assertions::assert_eq!(
            entities.iter().filter_map(|entity| entity.id().map(ToString::to_string)).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"]
        );
    }

    /// One source's answer holding `entities`.
    fn answered_by_source(entities: Vec<TemporalHistory>) -> ForwardedTemporalEntities {
        ForwardedTemporalEntities {
            registration_id: None,
            mode: ForwardingMode::Inclusive,
            entities,
            warnings: Vec::new(),
            linked_map: None,
            partial_ranges: Vec::new(),
        }
    }

    /// Clause 5.7.4.4 judges an entity over every part, so one a source
    /// answered only the in-effect probe for is exempt locally too.
    #[tokio::test]
    async fn probe_answers_join_the_contributed_set() {
        let forwarded = ForwardedHistory {
            sources: vec![answered_by_source(vec![history("urn:ngsi-ld:Vehicle:1").await])],
            budget: ResultBudget::new(ResultLimits {
                distinct_entities: NonZeroU32::MIN,
                source_rows: NonZeroU32::MIN,
            }),
        };
        let gathering = InEffectGathering {
            sources: vec![answered_by_source(vec![history("urn:ngsi-ld:Vehicle:2").await])],
            local: LocalInEffect::default(),
        };
        let params = TemporalQueryParameters {
            q: Some("speed>50".parse().expect("a valid q")),
            ..TemporalQueryParameters::default()
        };
        let contributed = contributed_entities(SplitEntities::Split, &params, Some(&forwarded), Some(&gathering), ContributedRendering::Local);
        assert!(contributed.contains(&id("urn:ngsi-ld:Vehicle:1")) && contributed.contains(&id("urn:ngsi-ld:Vehicle:2")));
        assert!(
            !contributed_entities(
                SplitEntities::Complete,
                &params,
                Some(&forwarded),
                Some(&gathering),
                ContributedRendering::Local
            )
            .contains(&id("urn:ngsi-ld:Vehicle:2"))
        );
    }

    /// Clause 5.7.4.4 judges the entities with data in the window (set S4):
    /// one the filters keep that no part answers for the window is never
    /// added, because the judgement only drops.
    #[tokio::test]
    async fn a_probe_only_entity_is_never_rendered() {
        let mut entities = vec![history("urn:ngsi-ld:Vehicle:1").await];
        let contributed = ContributedEntities::new([id("urn:ngsi-ld:Vehicle:1"), id("urn:ngsi-ld:Vehicle:2")], ContributedRendering::Local);
        let settlement = SplitSettlement {
            retained: [id("urn:ngsi-ld:Vehicle:1"), id("urn:ngsi-ld:Vehicle:2")].into_iter().collect(),
            incomplete: Vec::new(),
        };
        retain_settled(&mut entities, &contributed, &settlement);
        pretty_assertions::assert_eq!(
            entities.iter().filter_map(|entity| entity.id().map(ToString::to_string)).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1"]
        );
    }

    /// The query `raw` writes.
    fn query(raw: &str) -> TemporalQueryParameters {
        serde_urlencoded::from_str(raw).expect("a valid temporal query")
    }

    /// Clause 5.7.4.4: a representation that is no aggregate merges the
    /// holders' instances as they answered them, whatever the gathering.
    #[test]
    fn an_instance_representation_is_merged_as_answered() {
        for gathering in [AggregationGathering::EveryInstance, AggregationGathering::OverlapsAgain] {
            pretty_assertions::assert_eq!(
                ContributionShape::of(SplitEntities::Split, &query("timerel=after&timeAt=2024-01-01T00:00:00Z"), gathering),
                ContributionShape::AsAnswered
            );
        }
    }

    /// A split aggregated query carrying a value filter decides it over the
    /// merged instances, so every instance is gathered whatever the gathering.
    #[test]
    fn a_split_filtered_aggregate_always_gathers_every_instance() {
        let filtered = query("format=aggregatedValues&aggrMethods=totalCount&q=speed%3E1");
        for gathering in [AggregationGathering::EveryInstance, AggregationGathering::OverlapsAgain] {
            pretty_assertions::assert_eq!(
                ContributionShape::of(SplitEntities::Split, &filtered, gathering),
                ContributionShape::MergedInstances
            );
        }
    }

    /// An aggregated query deciding no filter after the merge aggregates its
    /// overlapping entities here, gathered as the deployment chose, split or
    /// not and filtered by each holder or not.
    #[test]
    fn an_aggregate_deciding_no_filter_after_the_merge_is_aggregated_here() {
        let queries = [
            (SplitEntities::Split, query("format=aggregatedValues&aggrMethods=totalCount")),
            (SplitEntities::Complete, query("format=aggregatedValues&aggrMethods=totalCount")),
            (SplitEntities::Complete, query("format=aggregatedValues&aggrMethods=totalCount&q=speed%3E1")),
        ];
        for (filtering, params) in &queries {
            for gathering in [AggregationGathering::EveryInstance, AggregationGathering::OverlapsAgain] {
                pretty_assertions::assert_eq!(
                    ContributionShape::of(*filtering, params, gathering),
                    ContributionShape::HubAggregated(gathering)
                );
            }
        }
    }

    /// Gathering every instance asks the sources for their instances and keeps
    /// their answers apart from the merge; gathering only the overlaps asks for
    /// their aggregates and merges them, as an unaggregated query does.
    #[test]
    fn each_gathering_reads_and_merges_the_sources_as_it_needs() {
        let every = ContributionShape::HubAggregated(AggregationGathering::EveryInstance);
        pretty_assertions::assert_eq!(every.reach(), HistoryReach::Complete);
        pretty_assertions::assert_eq!(every.remote_parts(), RemoteParts::RenderedApart);
        pretty_assertions::assert_eq!(every.rendering(), ContributedRendering::Local);
        let overlaps = ContributionShape::HubAggregated(AggregationGathering::OverlapsAgain);
        pretty_assertions::assert_eq!(overlaps.reach(), HistoryReach::AsAnswered);
        pretty_assertions::assert_eq!(overlaps.remote_parts(), RemoteParts::Merged);
        pretty_assertions::assert_eq!(overlaps.rendering(), ContributedRendering::Local);
    }
}
