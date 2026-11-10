//! The distributed part of a query: the two clause 5.7.2.4 forwarding phases
//! and what they bring back.

use crate::entity::{
    ContextSpec,
    ForwardQueryRequest,
    ProblemResponse,
    SourceContribution,
    collect_warnings,
    contributions_of,
    query_admission::AdmittedQuery,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_failure_classification::{
    forwarding_failure::local_federation_failure,
    result_limit_failure::{classify_instance_budget_refusal, classify_result_budget_refusal},
};
use andromeda_federation::{
    FederationService,
    forwarded_query::client_query::ClientQuery,
    matched_registrations::PhasedRegistrations,
    outcome::{ForwardReadOutcome, ForwardedEntities},
    query_forwarding_error::QueryForwardingError,
    request::{EntityMapInUse, QueryDelivery, TriggeringRequest},
    result_budget::{ResultBudget, ResultBudgetExceeded},
    split_entities::SplitEntities,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::declined_representation::single_source_declined;
use andromeda_linked_retrieval::narrowing::list::ListInForce;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::filter::expression::Expr;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::http::{HeaderMap, Uri as HttpUri};
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
    /// `list` is the request's `attrs`, `pick` or `omit`, which clauses 5.7.2
    /// and 4.21 apply after the merge.
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded`] when the merged collection exceeds the
    /// budget the sources were walked under: the local rows were read one past
    /// it, so a larger collection is refused rather than paged from a
    /// truncation (clause 4.12).
    pub(crate) fn merge_onto(self, local_entities: Vec<ReceivedState>, list: ListInForce<'_>) -> Result<MergedEntities, ResultBudgetExceeded> {
        let merged = if self.primary.is_empty() {
            local_entities
        } else {
            EntityQueryService::merge_with_forwarded(local_entities, self.primary)
        };
        let merged = if self.auxiliary.is_empty() {
            merged
        } else {
            EntityQueryService::merge_with_forwarded(merged, self.auxiliary)
        };
        let merged = EntityQueryService::narrow_merged(merged, list);
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
    /// The request URI problem responses name.
    pub(crate) uri: &'a HttpUri,
    /// The query as the client wrote it.
    pub(crate) query: &'a EntityQueryParameters,
    /// The query as admitted: the `Via` chain it arrived with, the
    /// `EntityMap` it runs under and the filters the registrations are
    /// selected by (clauses 4.9 and 5.10.2.4).
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
        // The expanded context source filter selects the registrations; the
        // raw expression is forwarded as the client wrote it (clause 5.7.2.4
        // propagates it into discovery).
        let csf_expr = self.admitted.registration_filters.csf();
        // Clause 4.12: one budget bounds everything the query gathers, from
        // both phases' sources and, once they have answered, the local store.
        let budget = ResultBudget::new(self.forwarding.result_limits());
        // Both phases match the same registrations, so they are read once.
        let phased = self.matched(csf_expr, &budget).await?;
        let (primary, auxiliary) = in_phases(phased, |registrations| self.phase(registrations, csf_expr, &budget), self.uri).await?;

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

    /// The forwarded query, as both phases send it.
    fn request<'b>(&self, csf_expr: Option<&'b Expr>, budget: &'b ResultBudget) -> ForwardQueryRequest<'b>
    where
        Self: 'b,
    {
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
            geo: self.admitted.registration_filters.geo(),
            entity_map: EntityMapInUse::from(&self.admitted.map_use),
            budget,
        }
    }

    /// Read the registrations the query matches, split by phase.
    ///
    /// One that could not be read because this broker's own store is saturated
    /// or unreachable fails the query rather than hide every source.
    async fn matched(&self, csf_expr: Option<&Expr>, budget: &ResultBudget) -> Result<PhasedRegistrations, ProblemResponse> {
        match self.forwarding.match_query(self.target, &self.request(csf_expr, budget)).await {
            Ok(matched) => Ok(matched.into_phases()),
            Err(error) => self.failed(error).map(|()| PhasedRegistrations::default()),
        }
    }

    /// One forwarding phase over the registrations it contacts.
    ///
    /// A forwarding that failed at its sources leaves the query with the local
    /// answer; a phase whose sources answer with more entities than the request
    /// may gather refuses the query, because a truncated collection would make
    /// its page window and `NGSILD-Results-Count` silently wrong (clauses 4.12
    /// and 6.3.13).
    async fn phase(
        &self,
        registrations: Vec<ContextSourceRegistration>,
        csf_expr: Option<&Expr>,
        budget: &ResultBudget,
    ) -> Result<ForwardReadOutcome<ForwardedEntities>, ProblemResponse> {
        match self.forwarding.forward_query(self.target, self.request(csf_expr, budget), registrations).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => self.failed(error).map(|()| ForwardReadOutcome::empty()),
        }
    }

    /// The failure of a forwarding step: a refusal, or the failure of this
    /// broker's own store, is the query's; a source that could not be reached
    /// is only logged.
    fn failed(&self, error: QueryForwardingError) -> Result<(), ProblemResponse> {
        match error {
            QueryForwardingError::TooManyResults(exceeded) => Err(ProblemResponse::classified(classify_result_budget_refusal(&exceeded), self.uri.clone())),
            QueryForwardingError::TooManyInstances(exceeded) => Err(ProblemResponse::classified(classify_instance_budget_refusal(&exceeded), self.uri.clone())),
            QueryForwardingError::Federation(error) => {
                if let Some(report) = local_federation_failure(&error) {
                    // clone: the problem response owns the request URI it names
                    return Err(ProblemResponse::classified(report, self.uri.clone()));
                }
                warn!(%error, "context source forwarding failed during the entity query");
                Ok(())
            }
        }
    }
}

/// Run the two clause 5.7.2.4 phases in order: the auxiliary sources are
/// contacted only once the first phase has been answered, so a query the first
/// phase refuses (clause 4.12) or that one proxied source declined (Table
/// 6.4.3.2-3) never reaches them.
///
/// # Errors
///
/// The failure of either phase, and the 501 of a lone proxied source.
async fn in_phases<Phase, Answered>(
    phased: PhasedRegistrations,
    mut phase: Phase,
    uri: &HttpUri,
) -> Result<(ForwardReadOutcome<ForwardedEntities>, ForwardReadOutcome<ForwardedEntities>), ProblemResponse>
where
    Phase: FnMut(Vec<ContextSourceRegistration>) -> Answered,
    Answered: Future<Output = Result<ForwardReadOutcome<ForwardedEntities>, ProblemResponse>>,
{
    let primary = phase(phased.primary).await?;
    if let Some(status) = primary.sources.single_source_status {
        return Err(single_source_declined(status, uri));
    }
    let auxiliary = phase(phased.auxiliary).await?;
    Ok((primary, auxiliary))
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{ProblemResponse, query_forwarding::in_phases},
        error::public_error,
    };
    use andromeda_federation::{
        matched_registrations::PhasedRegistrations,
        outcome::{ForwardReadOutcome, ForwardedEntities},
    };
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::http::Uri as HttpUri;
    use std::cell::Cell;

    fn uri() -> HttpUri {
        HttpUri::from_static("/ngsi-ld/v1/entities")
    }

    /// Clause 4.12: a first phase refused with `TooManyResults` leaves the
    /// auxiliary sources uncontacted.
    #[tokio::test]
    async fn a_refused_first_phase_never_starts_the_auxiliary_phase() {
        let uri = uri();
        let started = Cell::new(0_usize);

        let answer = in_phases(
            PhasedRegistrations::default(),
            |_registrations| {
                started.set(started.get() + 1);
                let refusal = public_error(NgsiLdErrorType::TooManyResults, "over the entity limit".to_owned(), &uri);
                async move { Err::<ForwardReadOutcome<ForwardedEntities>, _>(refusal) }
            },
            &uri,
        )
        .await;

        assert!(answer.is_err());
        pretty_assertions::assert_eq!(started.get(), 1);
    }

    /// Clause 4.3.6.2: an answered first phase is followed by the auxiliary one.
    #[tokio::test]
    async fn an_answered_first_phase_is_followed_by_the_auxiliary_phase() {
        let uri = uri();
        let started = Cell::new(0_usize);

        let answer = in_phases(
            PhasedRegistrations::default(),
            |_registrations| {
                started.set(started.get() + 1);
                async { Ok::<_, ProblemResponse>(ForwardReadOutcome::<ForwardedEntities>::empty()) }
            },
            &uri,
        )
        .await;

        assert!(answer.is_ok());
        pretty_assertions::assert_eq!(started.get(), 2);
    }
}
