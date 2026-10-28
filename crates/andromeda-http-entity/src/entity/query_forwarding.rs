//! The distributed part of a query: the two clause 5.7.2.4 forwarding phases
//! and what they bring back.

use crate::{
    entity::{
        ContextSpec,
        ContextualExpander,
        CsrModeFilter,
        EntityResultExt,
        ForwardQueryRequest,
        ProblemResponse,
        SourceContribution,
        collect_warnings,
        contributions_of,
        query_admission::AdmittedQuery,
    },
    error::public_error,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::{
    FederationService,
    forwarded_query::client_query::ClientQuery,
    outcome::{ForwardReadOutcome, ForwardedEntities},
    query_forwarding_error::QueryForwardingError,
    request::{EntityMapInUse, QueryDelivery, TriggeringRequest},
    result_budget::{ResultBudget, ResultBudgetExceeded},
    split_entities::SplitEntities,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::{declined_representation::single_source_declined, forwarding_failure::local_federation_failure};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_query::filter::{expansion::expand_csf_expr, expression::Expr};
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::http::{HeaderMap, Uri as HttpUri};
use iri_rs::IriBuf;
use tracing::warn;

/// What the two forwarding phases of a query brought back.
pub(crate) struct ForwardedQueryResults {
    /// The answers of the exclusive, redirect and inclusive sources.
    pub(crate) primary: Vec<ForwardedEntities>,
    /// The answers of the auxiliary sources (clause 4.3.6.2).
    pub(crate) auxiliary: Vec<ForwardedEntities>,
    /// The clause 6.3.17 warnings of both phases.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// What each source contributed to the map of the request (clause 5.2.39).
    pub(crate) contributions: Vec<SourceContribution>,
    /// The budget the sources were walked under, which bounds the merged
    /// collection and the local rows read for it too (clause 4.12).
    pub(crate) budget: ResultBudget,
}

impl ForwardedQueryResults {
    /// How many local rows the merge needs at most: one past the entity limit,
    /// which is all it takes to tell that the merged collection exceeds it.
    pub(crate) const fn local_row_limit(&self) -> u32 {
        self.budget.limits().distinct_entities.get().saturating_add(1)
    }

    /// Whether any source returned an entity, which is what makes the local
    /// page an incomplete view of the result (clause 5.5.9).
    pub(crate) fn holds_remote_entities(&self) -> bool {
        self.primary.iter().chain(&self.auxiliary).any(|result| !result.entities.is_empty())
    }

    /// Fold what the sources returned onto the entities the broker holds
    /// itself: the primary phase's answers first, then the auxiliary ones,
    /// which only supplement what is present already (clause 4.3.6.2).
    /// `attrs` holds the expanded names of the query's `attrs`, which clause
    /// 5.7.2 applies after the merge.
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded`] when the merged collection exceeds the
    /// budget the sources were walked under: the local rows were read one past
    /// it, so a larger collection is refused rather than paged from a
    /// truncation (clause 4.12).
    pub(crate) fn merge_onto(self, local_entities: Vec<ReceivedState>, attrs: &[IriBuf]) -> Result<MergedEntities, ResultBudgetExceeded> {
        let merged = if self.primary.is_empty() {
            local_entities
        } else {
            EntityQueryService::merge_with_forwarded(local_entities, self.primary, attrs)
        };
        let merged = if self.auxiliary.is_empty() {
            merged
        } else {
            EntityQueryService::merge_with_forwarded(merged, self.auxiliary, attrs)
        };
        self.budget.admit_collection(u64::try_from(merged.len()).unwrap_or(u64::MAX))?;
        Ok(MergedEntities {
            entities: merged,
            warnings: self.warnings,
            contributions: self.contributions,
        })
    }
}

/// The entities a query answers with before its page is cut, together with
/// what the forwarding reported about the sources that helped.
pub(crate) struct MergedEntities {
    /// The local and forwarded entities, merged on the model.
    pub(crate) entities: Vec<ReceivedState>,
    /// The clause 6.3.17 warnings of the forwarding phases.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// What each source contributed to the map of the request (clause 5.2.39).
    pub(crate) contributions: Vec<SourceContribution>,
}

impl MergedEntities {
    /// The answer of a query no context source contributed to.
    pub(crate) const fn local(entities: Vec<ReceivedState>) -> MergedEntities {
        MergedEntities {
            entities,
            warnings: Vec::new(),
            contributions: Vec::new(),
        }
    }
}

/// The two clause 5.7.2.4 forwarding phases of one query: the exclusive,
/// redirect and inclusive sources first, then the auxiliary ones (clause
/// 4.3.6.2).
pub(crate) struct QueryForwarding<'a> {
    /// The forwarding policy.
    pub(crate) forwarding: &'a FederationService,
    /// Live or Snapshot branch selected for the request.
    pub(crate) target: &'a ContextTarget,
    /// The JSON-LD engine the context source filter is expanded with.
    pub(crate) engine: &'a JsonLdEngine,
    /// The request URI problem responses name.
    pub(crate) uri: &'a HttpUri,
    /// The query as the client wrote it.
    pub(crate) query: &'a EntityQueryParameters,
    /// The query as admitted: the `Via` chain it arrived with, the
    /// `EntityMap` it runs under and the geoquery the registrations are
    /// selected by (clause 5.10.2.4).
    pub(crate) admitted: &'a AdmittedQuery,
    /// The resolved split-entities policy.
    pub(crate) split: SplitEntities,
    /// The request's JSON-LD context.
    pub(crate) context: &'a ContextSpec,
    /// The headers of the triggering request, passed through to the sources
    /// (clause 6.3.19).
    pub(crate) headers: &'a HeaderMap,
    /// The binding the client's query arrived on, mirrored by the forward when
    /// the registration advertises it (clause 6.23.1).
    pub(crate) delivery: QueryDelivery,
}

impl QueryForwarding<'_> {
    /// Run both phases.
    ///
    /// A source that could not be reached is a clause 6.3.17 warning, never a
    /// failure of the query; only the 501 of a lone proxied source is (Table
    /// 6.4.3.2-3), and so are answers exceeding the request's budget (clause
    /// 4.12, `TooManyResults`).
    pub(crate) async fn run(&self) -> Result<ForwardedQueryResults, ProblemResponse> {
        // The CSR filter was parsed while extracting the request. The expanded
        // copy selects the registrations; the raw expression is forwarded as
        // the client wrote it (clause 5.7.2.4 propagates it into discovery).
        //
        // No `jsonKeys` declaration applies: Table 5.2.12-1 scopes that member
        // to Entity attribute names, while `csf` addresses the Context Source
        // Properties of a registration (clause 4.9), whose values are raw JSON
        // addressed by their literal keys.
        let csf_expr: Option<Expr> = match &self.query.csf {
            Some(expr) => {
                let prepared_context = self.engine.prepare_term_context(self.context);
                let csf_expander = ContextualExpander::new(self.engine, &prepared_context);
                Some(expand_csf_expr(expr.clone(), &csf_expander).await.with_entity_instance(self.uri)?) // clone: the raw filter stays in the query for forwarding
            }
            None => None,
        };
        // Clause 4.12: one budget bounds everything the query gathers, from
        // both phases' sources and, once they have answered, the local store.
        let budget = ResultBudget::new(self.forwarding.result_limits());
        let primary = self.phase(CsrModeFilter::ExcludeAuxiliary, csf_expr.as_ref(), &budget).await?;
        if let Some(status) = primary.sources.single_source_status {
            return Err(single_source_declined(status, self.uri));
        }
        let auxiliary = self.phase(CsrModeFilter::AuxiliaryOnly, csf_expr.as_ref(), &budget).await?;

        let mut warnings = collect_warnings(&primary.results);
        warnings.extend(collect_warnings(&auxiliary.results));
        // Capture which registration returned which entity, and with which
        // map, before the merge (clauses 5.2.39 and 5.5.14).
        let mut contributions: Vec<SourceContribution> = contributions_of(&primary.results).collect();
        contributions.extend(contributions_of(&auxiliary.results));
        Ok(ForwardedQueryResults {
            primary: primary.results,
            auxiliary: auxiliary.results,
            warnings,
            contributions,
            budget,
        })
    }

    /// One forwarding phase over the registrations `mode_filter` admits.
    ///
    /// A forwarding that failed at its sources leaves the query with the local
    /// answer, but one that could not read the registrations to forward to,
    /// because this broker's own store is saturated or unreachable, fails the
    /// query rather than hide every source; a phase whose sources answer with more entities than the request
    /// may gather refuses the query, because a truncated collection would make
    /// its page window and `NGSILD-Results-Count` silently wrong (clauses 4.12
    /// and 6.3.13).
    async fn phase(
        &self,
        mode_filter: CsrModeFilter,
        csf_expr: Option<&Expr>,
        budget: &ResultBudget,
    ) -> Result<ForwardReadOutcome<ForwardedEntities>, ProblemResponse> {
        let forwarded = self
            .forwarding
            .forward_query(
                self.target,
                ForwardQueryRequest {
                    client_query: ClientQuery::EntityQuery(self.query),
                    delivery: self.delivery,
                    split: self.split,
                    context: self.context,
                    triggering: TriggeringRequest {
                        via: self.admitted.incoming_via.as_ref(),
                        headers: Some(self.headers),
                    },
                    csf_expr,
                    geo: self.admitted.registry_geo.as_ref(),
                    mode_filter,
                    entity_map: EntityMapInUse::from(&self.admitted.map_use),
                    budget,
                },
            )
            .await;
        match forwarded {
            Ok(outcome) => Ok(outcome),
            Err(QueryForwardingError::TooManyResults(exceeded)) => Err(public_error(NgsiLdErrorType::TooManyResults, exceeded.to_string(), self.uri)),
            Err(QueryForwardingError::TooManyInstances(exceeded)) => Err(public_error(NgsiLdErrorType::TooManyResults, exceeded.to_string(), self.uri)),
            Err(QueryForwardingError::Federation(error)) => {
                if let Some(report) = local_federation_failure(&error) {
                    // clone: the problem response owns the request URI it names
                    return Err(report.with_instance(self.uri.clone()));
                }
                warn!(%error, "context source forwarding failed during the entity query");
                Ok(ForwardReadOutcome::empty())
            }
        }
    }
}
