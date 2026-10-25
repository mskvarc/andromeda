//! Asking the registered context sources for their part of a queried history
//! (clauses 5.7.4.4 and 6.3.18).

use crate::temporal::{
    ContextSpec,
    ContextualExpander,
    CsrModeFilter,
    ForwardTemporalQueryRequest,
    ForwardingOperation,
    ProblemResponse,
    ResultInstanceExt,
    TemporalQueryParameters,
    query::admission::AdmittedQuery,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_federation::{
    FederationService,
    config::SplitEntities,
    entity_budget::{EntityBudget, EntityBudgetExceeded},
    in_effect_probe::InEffectProbe,
    instance_budget::InstanceBudget,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities},
    query_forwarding_error::QueryForwardingError,
    request::{EntityMapInUse, HistoryBudget, SourcePaging, TriggeringRequest},
};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    declined_representation::single_source_declined,
    forwarding_failure::local_federation_failure,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::{
    filter::{expansion::expand_csf_expr, expression::Expr, language_targeting::LanguageTargeting},
    geo::query::GeoQuery,
};
use axum::http::{HeaderMap, Uri};
use tracing::warn;

/// How far a temporal query reaches (clause 5.5.13 `local`).
///
/// Clause 5.5.13 lets a request be "limited to the local scope", and a Snapshot
/// branch has no distributed reading at all, so both answers are decided here
/// once and the forwarding reads that answer rather than a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryScope {
    /// The query stays inside this broker; no context source is contacted.
    Local,
    /// The query is spread over every matching registration.
    Distributed,
}

impl QueryScope {
    /// How far `params` reaches against `target`.
    pub(crate) const fn of(target: &ContextTarget, params: &TemporalQueryParameters) -> QueryScope {
        if target.permits_federation() && !params.is_local() {
            QueryScope::Distributed
        } else {
            QueryScope::Local
        }
    }

    /// Whether an entity's attributes may be met spread over several sources.
    ///
    /// Clause 5.7.4.4: only a distributed read can meet a split entity, so a
    /// local one always sees complete entities and defers no filter.
    pub(crate) const fn splitting(self, requested: SplitEntities) -> SplitEntities {
        match self {
            QueryScope::Local => SplitEntities::Complete,
            QueryScope::Distributed => requested,
        }
    }
}

/// How much of each source's history a temporal query reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryReach {
    /// Each source's answer as it is, in the representation the client asked
    /// for.
    AsAnswered,
    /// Each source's instances of the whole window, whatever representation
    /// the client asked for: an aggregate of an entity held in several parts
    /// is one of its merged instances (clause 5.7.4.4).
    Complete,
}

/// How a temporal query reaches the context sources that hold the rest of the
/// history: the federation service that contacts them, the triggering request
/// every forwarded copy replays (clause 6.3.18), the `splitEntities` mode they
/// are asked under (clause 5.7.4.4), and the Entity Map and the geoquery that
/// fix which of them are contacted (clauses 5.5.14 and 5.10.2.4).
#[derive(Clone, Copy)]
pub(crate) struct TemporalForwarding<'a> {
    /// The service that contacts every matching registration.
    pub(crate) forwarding: &'a FederationService,
    /// The triggering request every forwarded copy replays, so a forwarding
    /// loop is detected rather than followed.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// Whether an entity's attributes may be spread over several sources.
    pub(crate) split: SplitEntities,
    /// The Entity Map the query runs under.
    pub(crate) map_use: &'a EntityMapUse,
    /// The query's geoquery, compiled, which a registration's declared
    /// geometry has to admit for it to be contacted.
    pub(crate) geo: Option<&'a GeoQuery>,
    /// How much of each source's history is read.
    pub(crate) reach: HistoryReach,
}

impl<'a> TemporalForwarding<'a> {
    /// The forwarding of the `admitted` query, arrived with `headers` and
    /// asked under `split`, reading `reach` of each source's history.
    pub(super) const fn new(
        forwarding: &'a FederationService,
        admitted: &'a AdmittedQuery,
        headers: &'a HeaderMap,
        split: SplitEntities,
        reach: HistoryReach,
    ) -> TemporalForwarding<'a> {
        TemporalForwarding {
            forwarding,
            triggering: TriggeringRequest {
                via: admitted.incoming_via.as_ref(),
                headers: Some(headers),
            },
            split,
            map_use: &admitted.map_use,
            geo: admitted.registry_geo.as_ref(),
            reach,
        }
    }
}

/// What the context sources answered for their part of a queried history, and
/// the budget their answers were gathered under.
pub(crate) struct ForwardedHistory {
    /// One entry per contacted source, each holding every page of its answer.
    pub(crate) sources: Vec<ForwardedTemporalEntities>,
    /// The budget the sources were walked under, which bounds the merged
    /// collection and the local rows read for it too (clause 4.12).
    pub(crate) budget: EntityBudget,
}

impl ForwardedHistory {
    /// Whether any source returned an entity, which is what makes the local
    /// page an incomplete view of the result (clause 5.5.9).
    pub(crate) fn holds_remote_entities(&self) -> bool {
        self.sources.iter().any(|source| !source.entities.is_empty())
    }

    /// How many local rows the merge needs at most: one past the budget, which
    /// is all it takes to tell that the merged collection exceeds it.
    pub(crate) const fn local_row_limit(&self) -> u32 {
        self.budget.maximum().get().saturating_add(1)
    }
}

/// The clause 4.12 `TooManyResults` refusal of a query whose sources, or whose
/// merged collection, exceed what one request gathers.
pub(crate) fn too_many_results(exceeded: EntityBudgetExceeded, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(NgsiLdErrorType::TooManyResults, exceeded.to_string()).with_instance(instance.clone())
}

/// Ask every matching context source for its part of the history, unless the
/// query stays inside this broker.
///
/// Each source's answer is read to its last page (clauses 4.3.6.7 and
/// 5.5.9.1), because the client's page is cut from the whole merge; one budget
/// bounds what all of them gather together (clause 4.12). The context source
/// filter selects registrations in its expanded form; the raw expression
/// travels on as the client wrote it, since clause 5.7.4.4 propagates it into
/// discovery. A source that cannot be reached contributes a clause 6.3.17
/// warning rather than failing the operation. A clause 5.5.13 local query
/// contacts nothing and answers with no contribution at all, which is what the
/// absent result means.
///
/// # Errors
///
/// The classified failure of expanding the context source filter, the clause
/// 4.12 `TooManyResults` refusal when the sources' answers exceed the budget,
/// and the Table 6.4.3.2-3 `NotImplemented` answer when the one source holding
/// all the data declined the requested representation.
pub(crate) async fn forward_temporal_history(
    sources: TemporalForwarding<'_>,
    scope: QueryScope,
    engine: &JsonLdEngine,
    target: &ContextTarget,
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    instance: &Uri,
) -> Result<Option<ForwardedHistory>, ProblemResponse> {
    let TemporalForwarding {
        forwarding,
        triggering,
        split,
        map_use,
        geo,
        reach,
    } = sources;
    if scope == QueryScope::Local {
        return Ok(None);
    }
    let csf_expr = expanded_csf(engine, params, context, instance).await?;
    let budget = EntityBudget::new(forwarding.max_query_entities());
    let instances = InstanceBudget::new(forwarding.max_query_instances());
    let paging = match reach {
        HistoryReach::AsAnswered => SourcePaging::Walk(&budget),
        HistoryReach::Complete => SourcePaging::WalkHistory(HistoryBudget {
            entities: &budget,
            instances: &instances,
        }),
    };
    let forwarded = forwarding
        .forward_temporal_query(
            target,
            ForwardTemporalQueryRequest {
                client_query: params,
                split,
                context,
                operation: ForwardingOperation::QueryTemporalEntity,
                triggering,
                csf_expr: csf_expr.as_ref(),
                geo,
                mode_filter: CsrModeFilter::All,
                entity_map: EntityMapInUse::from(map_use),
                paging,
            },
        )
        .await;
    let outcome = forwarding_outcome(forwarded, instance)?;
    match outcome.sources.single_source_status {
        Some(status) => Err(single_source_declined(status, instance)),
        None => Ok(Some(ForwardedHistory {
            sources: outcome.results,
            budget,
        })),
    }
}

/// The context source filter of `params` in its expanded form, which selects
/// registrations; the raw expression travels on as the client wrote it.
///
/// No `jsonKeys` declaration applies to `csf`: Table 5.2.12-1 scopes that
/// member to Entity attribute names, while `csf` addresses the Context Source
/// Properties of a registration (clause 4.9), whose values are raw JSON
/// addressed by their literal keys.
///
/// # Errors
///
/// The classified failure of expanding the filter.
async fn expanded_csf(engine: &JsonLdEngine, params: &TemporalQueryParameters, context: &ContextSpec, instance: &Uri) -> Result<Option<Expr>, ProblemResponse> {
    let Some(expr) = &params.csf else {
        return Ok(None);
    };
    let prepared_context = engine.prepare_term_context(context);
    let expander = ContextualExpander::new(engine, &prepared_context);
    Ok(Some(expand_csf_expr(expr.clone(), &expander).await.with_instance(instance)?)) // clone: the raw filter stays in the query for forwarding
}

/// The outcome of one forwarded temporal read, or the failure that ends the
/// query.
///
/// A truncated collection would make the page window and
/// `NGSILD-Results-Count` silently wrong, so exceeding the budget refuses the
/// query (clauses 4.12 and 6.3.13); a forwarding that failed at its sources
/// leaves the query with the local answer, but one that could not read the
/// registrations because this broker's store is saturated or unreachable
/// fails it rather than hide every source.
///
/// # Errors
///
/// The clause 4.12 `TooManyResults` refusal, and the classified failure of a
/// forwarding this broker's own store failed.
fn forwarding_outcome(
    forwarded: Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError>,
    instance: &Uri,
) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, ProblemResponse> {
    match forwarded {
        Ok(outcome) => Ok(outcome),
        Err(QueryForwardingError::TooManyResults(exceeded)) => Err(too_many_results(exceeded, instance)),
        // clone: the problem response owns the request URI it names
        Err(QueryForwardingError::TooManyInstances(exceeded)) => {
            Err(ClassifiedReport::public(NgsiLdErrorType::TooManyResults, exceeded.to_string()).with_instance(instance.clone()))
        }
        Err(QueryForwardingError::Federation(error)) => {
            if let Some(report) = local_federation_failure(&error) {
                // clone: the problem response owns the request URI it names
                return Err(report.with_instance(instance.clone()));
            }
            warn!(%error, "context source forwarding failed during the temporal query");
            Ok(ForwardReadOutcome::empty())
        }
    }
}

/// Ask every matching context source, once per probe of `probes`, for the
/// instances the filters of the query read in effect when its window opens
/// (clauses 4.11 and 5.7.4.4).
///
/// Each probe walks every page of every source's answer under a budget of
/// its own, the size of the window query's (clause 4.12), and a source that
/// cannot be reached contributes a clause 6.3.17 warning, as for the window.
///
/// # Errors
///
/// The classified failure of expanding the context source filter, the clause
/// 4.12 `TooManyResults` refusal when a probe's answers exceed its budget, and
/// the classified failure of a forwarding this broker's own store failed.
pub(crate) async fn forward_in_effect_probes(
    sources: TemporalForwarding<'_>,
    engine: &JsonLdEngine,
    target: &ContextTarget,
    request: InEffectRequest<'_>,
    probes: &[InEffectProbe<'_>],
) -> Result<Vec<ForwardedTemporalEntities>, ProblemResponse> {
    let TemporalForwarding {
        forwarding,
        triggering,
        split,
        map_use,
        geo,
        reach: _,
    } = sources;
    // The probes forward the client's `q` as written, which each source reads
    // as its own configuration does; the negotiated level decides this
    // broker's reading alone.
    let InEffectRequest {
        params,
        context,
        language_targeting: _,
        instance,
    } = request;
    let csf_expr = expanded_csf(engine, params, context, instance).await?;
    let mut answers = Vec::new();
    for probe in probes {
        let budget = EntityBudget::new(forwarding.max_query_entities());
        let forwarded = forwarding
            .forward_in_effect_probe(
                target,
                ForwardTemporalQueryRequest {
                    client_query: params,
                    split,
                    context,
                    operation: ForwardingOperation::QueryTemporalEntity,
                    triggering,
                    csf_expr: csf_expr.as_ref(),
                    geo,
                    mode_filter: CsrModeFilter::All,
                    entity_map: EntityMapInUse::from(map_use),
                    paging: SourcePaging::Walk(&budget),
                },
                *probe,
            )
            .await;
        answers.extend(forwarding_outcome(forwarded, instance)?.results);
    }
    Ok(answers)
}

/// The query the in-effect probes are derived from.
#[derive(Clone, Copy)]
pub(crate) struct InEffectRequest<'r> {
    /// The client's query.
    pub(crate) params: &'r TemporalQueryParameters,
    /// The request context.
    pub(crate) context: &'r ContextSpec,
    /// What the query's target languages may be and how they select
    /// languageMap keys.
    pub(crate) language_targeting: LanguageTargeting,
    /// The request URI a failure names.
    pub(crate) instance: &'r Uri,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::query::forwarding::{QueryScope, forwarding_outcome};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_federation::{config::SplitEntities, entity_budget::EntityBudget, outcome::ForwardReadOutcome, query_forwarding_error::QueryForwardingError};
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
    use axum::{
        http::{StatusCode, Uri},
        response::IntoResponse,
    };
    use std::num::NonZeroU32;

    /// The tenant's ordinary live branch, which a distributed read may leave.
    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    /// A temporal query confined to this broker (clause 5.5.13 `local=true`).
    fn local_query() -> TemporalQueryParameters {
        TemporalQueryParameters {
            local: Some(true),
            ..TemporalQueryParameters::default()
        }
    }

    /// Clause 5.5.13: a request limited to the local scope contacts no context
    /// source, whatever registrations would otherwise match.
    #[test]
    fn a_local_query_never_leaves_this_broker() {
        pretty_assertions::assert_eq!(QueryScope::of(&live_target(), &local_query()), QueryScope::Local);
    }

    /// A query that did not ask for the local scope reaches every matching
    /// registration (clause 5.7.4.4).
    #[test]
    fn a_query_that_did_not_ask_for_the_local_scope_is_distributed() {
        pretty_assertions::assert_eq!(QueryScope::of(&live_target(), &TemporalQueryParameters::default()), QueryScope::Distributed);
    }

    /// Clause 5.7.4.4: only a distributed read can meet an entity whose
    /// attributes are spread over several sources, so a local one always sees
    /// complete entities however the request asked.
    #[test]
    fn a_local_query_always_sees_complete_entities() {
        pretty_assertions::assert_eq!(QueryScope::Local.splitting(SplitEntities::Split), SplitEntities::Complete);
        pretty_assertions::assert_eq!(QueryScope::Distributed.splitting(SplitEntities::Split), SplitEntities::Split);
        pretty_assertions::assert_eq!(QueryScope::Distributed.splitting(SplitEntities::Complete), SplitEntities::Complete);
    }

    /// Clause 4.12: a probe whose sources answer more entities than one
    /// request gathers refuses the query as the window query does, with
    /// `TooManyResults` (403).
    #[test]
    fn a_probe_over_its_budget_is_too_many_results() {
        let budget = EntityBudget::new(NonZeroU32::MIN);
        let exceeded = budget.charge(2).expect_err("two entities exceed a budget of one");
        let instance: Uri = "/ngsi-ld/v1/temporal/entities".parse().expect("a valid URI");
        let Err(refusal) = forwarding_outcome(Err(QueryForwardingError::TooManyResults(exceeded)), &instance) else {
            panic!("a probe over its budget is refused");
        };
        pretty_assertions::assert_eq!(refusal.into_response().status(), StatusCode::FORBIDDEN);
    }

    /// A forwarding that succeeded hands its outcome on unchanged.
    #[test]
    fn a_forwarded_outcome_is_handed_on() {
        let instance: Uri = "/ngsi-ld/v1/temporal/entities".parse().expect("a valid URI");
        let Ok(outcome) = forwarding_outcome(Ok(ForwardReadOutcome::empty()), &instance) else {
            panic!("the outcome is handed on");
        };
        assert!(outcome.results.is_empty());
    }
}
