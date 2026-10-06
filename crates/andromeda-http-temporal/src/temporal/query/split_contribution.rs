//! The entities a Context Source contributed a part of to a split temporal
//! query, and the filters decided over their gathered parts (clause 5.7.4.4).
//!
//! Under `splitEntities` the value, geo and scope filters are applied "after
//! the Entity information from different Context Sources and local
//! information, if there is any, has been aggregated". An entity no source
//! answered with has no part outside this broker, so the local query keeps
//! deciding it in the store; only the entities a source answered with are
//! exempt locally and judged here, over every part, once the sources have
//! answered.

use crate::temporal::{
    ProblemResponse,
    ResultInstanceExt,
    TemporalQueryParameters,
    query::{
        forwarding::{ForwardedHistory, HistoryReach},
        merged_history::{MergedTemporalHistory, RemoteParts, merge_forwarded_history},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::config::SplitEntities;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::contributed_entities::{ContributedEntities, ContributedRendering};
use andromeda_temporal::{service::TemporalService, split_settlement::SplitSettlement};
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
}

impl ContributionShape {
    /// The shape `params` takes under `filtering`.
    pub(crate) fn of(filtering: SplitEntities, params: &TemporalQueryParameters) -> ContributionShape {
        let aggregated = params.parse_format().is_ok_and(|format| format.is_aggregated());
        match filtering {
            SplitEntities::Split if aggregated && carries_split_filters(params) => ContributionShape::MergedInstances,
            SplitEntities::Split | SplitEntities::Complete => ContributionShape::AsAnswered,
        }
    }

    /// How much of each source's history is read.
    pub(crate) const fn reach(self) -> HistoryReach {
        match self {
            ContributionShape::AsAnswered => HistoryReach::AsAnswered,
            ContributionShape::MergedInstances => HistoryReach::Complete,
        }
    }

    /// How the local read renders a contributed entity.
    pub(crate) const fn rendering(self) -> ContributedRendering {
        match self {
            ContributionShape::AsAnswered => ContributedRendering::Local,
            ContributionShape::MergedInstances => ContributedRendering::Merged,
        }
    }

    /// Whether the sources' entities join the local ones.
    pub(crate) const fn remote_parts(self) -> RemoteParts {
        match self {
            ContributionShape::AsAnswered => RemoteParts::Merged,
            ContributionShape::MergedInstances => RemoteParts::RenderedApart,
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
    let mut parts: Vec<(Identifier, Vec<TemporalDocument>)> = settlement.retained.iter().map(|entity_id| (entity_id.clone(), Vec::new())).collect(); // clone: each part list owns the identifier it is rendered under
    for answer in answered_documents(history) {
        let Some(entity_id) = answer.id() else {
            continue;
        };
        if let Some((_, answers)) = parts.iter_mut().find(|(held, _)| held == entity_id) {
            // clone: the sources' answers stay with the history their warnings and contributions are read from
            answers.push(answer.clone());
        }
    }
    let rendering = service
        .render_merged_aggregations(request.target, request.params, request.context, parts)
        .await
        .with_instance(instance)?;
    let warnings = rendering
        .warnings
        .into_iter()
        .map(|warning| NgsiLdWarning::RevalidationFailed(format!("{}: {}", warning.entity_id, warning.message)))
        .collect();
    Ok(Some((rendering.entities, warnings)))
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
    /// What the sources answered.
    forwarded: Option<&'r ForwardedHistory>,
}

/// Whether the query carries any of the filters clause 5.7.4.4 applies after
/// aggregation; without one, S7 is S4 and nothing is decided.
pub(crate) const fn carries_split_filters(params: &TemporalQueryParameters) -> bool {
    params.q.is_some() || params.geo.0.is_some() || params.scope_q.is_some()
}

/// The entities a source answered a split query with, which the local read
/// exempts from the value, geo and scope filters.
///
/// Empty unless the query is split, carries one of those filters, and a
/// source answered: every other query reads exactly as one that was never
/// split.
pub(crate) fn contributed_entities(
    filtering: SplitEntities,
    params: &TemporalQueryParameters,
    forwarded: Option<&ForwardedHistory>,
    rendering: ContributedRendering,
) -> ContributedEntities {
    let Some(history) = forwarded else {
        return ContributedEntities::default();
    };
    match filtering {
        SplitEntities::Split if carries_split_filters(params) => {
            ContributedEntities::new(answered_parts(history).filter_map(|answer| answer.id().cloned()), rendering)
        } // clone: the set owns the identifiers it exempts
        SplitEntities::Split | SplitEntities::Complete => ContributedEntities::default(),
    }
}

/// Every entity part any source answered with.
fn answered_parts(history: &ForwardedHistory) -> impl Iterator<Item = &TemporalHistory> {
    history.sources.iter().flat_map(|source| source.entities.iter())
}

/// Every part any source answered with its instances; an aggregate holds none
/// to judge or to aggregate again.
fn answered_documents(history: &ForwardedHistory) -> impl Iterator<Item = &TemporalDocument> {
    answered_parts(history).filter_map(|answer| match answer {
        TemporalHistory::Instances(document) => Some(document),
        TemporalHistory::Aggregates(_) => None,
    })
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
    instance: &Uri,
) -> Result<MergedTemporalHistory, ProblemResponse> {
    let settled = SettledRequest {
        target: merge.target,
        params: merge.params,
        context: merge.context,
        forwarded: forwarded.as_ref(),
    };
    let settlement = settle_contributions(service, settled, merge.contributed, instance).await?;
    let rendered = render_contributions(service, settled, merge.shape, settlement.as_ref(), instance).await?;
    let mut merged = merge_forwarded_history(
        local_entities,
        forwarded,
        merge.filtering,
        merge.shape.remote_parts(),
        merge.params.timeproperty_or_default(),
        instance,
    )?;
    if let Some(settlement) = settlement {
        retain_settled(&mut merged.entities, merge.contributed, &settlement);
    }
    if let Some((entities, warnings)) = rendered {
        merged.entities.extend(entities);
        merged.warnings.extend(warnings);
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
    let answered: Vec<&TemporalDocument> = answered_documents(history).collect();
    let settlement = service
        .settle_split_filters(request.target, request.params, request.context, &answered)
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
mod tests {
    use crate::{
        temporal::query::split_contribution::{carries_split_filters, retain_settled},
        test_history::held_history,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
    use andromeda_ngsi_ld_query::{
        scope::query::ScopeQuery,
        temporal::contributed_entities::{ContributedEntities, ContributedRendering},
    };
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
    use andromeda_temporal::split_settlement::SplitSettlement;
    use serde_json::json;

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
        };
        retain_settled(&mut entities, &contributed, &settlement);
        pretty_assertions::assert_eq!(
            entities.iter().filter_map(|entity| entity.id().map(ToString::to_string)).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"]
        );
    }
}
