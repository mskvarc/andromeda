//! Capturing the result of one snapshot entity query (clause 5.16.1.4).
//!
//! Clause 5.16.1.4 executes a Snapshot's entity query "following the behaviour
//! described in clause 5.7.2.4. If the size of the respective results require
//! pagination, all pages are to be retrieved completely". The local store is
//! read page by page; every matching Context Source is walked once, to the last
//! page of its answer, before the first local page is written, because a
//! source's own pages carry no relation to the local ones: a source entity is
//! merged onto the local page that holds the same entity, and every source
//! entity no local page held is captured after the last one.
//!
//! Under `splitEntities`, once a source answered an entity, the local pages
//! are read without `q`, the geoquery, `scopeQ` and the Attribute list, each
//! entity whole, as the sources are asked without them, and the query decides
//! on each entity once it is merged (clauses 4.3.6 and 5.7.2.4). Every
//! captured entity is reduced to the query's `attrs`, `pick` or `omit`, as
//! Query Entities answers it (clause 5.7.2.5).
//!
//! A local entity is captured as the stored state the query selected, written
//! into the Snapshot's branch as stored. Only an entity a source also answered
//! for, or one only a source holds, is written anew at the capture instant.
//!
//! The entity limit the sources were walked under holds for the whole capture
//! once a source answered an entity: Query Entities then refuses a merged
//! collection past it (clause 4.12), so each local page is checked against it
//! before it is written. A capture no source answered an entity for pages its
//! local entities unbounded, as Query Entities pages them.
//!
//! A query that fails as Query Entities would records the problem Query
//! Entities answers with, and a failure only the capture meets is recorded as
//! the capture's own. A Context Source that fails does not fail the query, as
//! it does not fail Query Entities: the capture holds what the broker and the
//! other sources answered, and its execution result names the source.
//!
//! A failure met after a page was written leaves that page in the staging
//! branch: the query's capture says it wrote, and the Snapshot is captured
//! again without the query.

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        captured_page::{CandidateEntity, CaptureSelection},
        execution::entity_query_failure,
        forwarded_capture::ForwardedCapture,
        page_outcome::PageOutcome,
        pending_sources::PendingSources,
        query_capture::{QueryCapture, Staging},
        source_shortfall::SourceShortfall,
        status::{answered_detail, failure_detail},
        translation::entity_parameters,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::{EntityQueryService, answer_list::AnswerList, local_read::LocalRead};
use andromeda_failure_classification::{
    forwarding_failure::classify_federation_error,
    parameter_failure::classify_parameter_error,
    query_failure::classify_query_error,
    result_limit_failure::{classify_instance_budget_refusal, classify_result_budget_refusal},
};
use andromeda_federation::{
    error::FederationError,
    expanded_registration_filters::ExpandedRegistrationFilters,
    forwarded_query::client_query::ClientQuery,
    instance_budget::InstanceBudgetExceeded,
    query_forwarding_error::QueryForwardingError,
    request::{EntityMapInUse, ForwardQueryRequest, QueryDelivery, TriggeringRequest},
    result_budget::{ResultBudget, ResultBudgetExceeded},
    split_entities::SplitEntities,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    query::body::Query,
    snapshot::{ExecutionResultDetails, ExecutionResultStatus},
};
use andromeda_ngsi_ld_query::{
    candidate_filters::CandidateFilters,
    page_window::{PageStart, PageWindow},
    query_error::QueryError,
};
use andromeda_ngsi_ld_request::query::entity::{operation::EntityQueryOperation, parameters::EntityQueryParameters};
use chrono::Utc;
use std::collections::HashSet;
use tracing::warn;

/// What walking every matching Context Source came to.
enum WalkedSources {
    /// Every source answered, or failed with a warning of its own.
    Answered(PendingSources),
    /// The forwarding could not run at all.
    Unreachable(FederationError),
    /// The sources answered with more distinct entities, or more rows, than
    /// one query gathers.
    TooManyResults(ResultBudgetExceeded),
    /// The sources answered with more attribute instances than one query
    /// gathers.
    TooManyInstances(InstanceBudgetExceeded),
}

/// One snapshot entity query, as every page of its capture writes it.
struct EntityCapture<'q> {
    /// The Snapshot's staging branch.
    target: &'q ContextTarget,
    /// The Snapshot being prepared.
    snapshot_id: &'q Identifier,
    /// The query's position in `snapshotQueries`.
    index: usize,
    /// What selects and narrows the entities the query captures.
    selection: CaptureSelection<'q>,
    /// How the entities its Context Sources contributed to are written.
    forwarded: ForwardedCapture,
}

/// The identifiers a capture has written, for the query and across the
/// Snapshot.
struct CaptureLedger<'a> {
    /// Every entity this query captured.
    captured: HashSet<Identifier>,
    /// Every entity any query of the Snapshot has materialized so far.
    materialized: &'a mut HashSet<Identifier>,
    /// Whether this query wrote into the staging branch.
    staging: Staging,
}

impl SnapshotExecutor {
    /// Execute one snapshot entity query and capture its whole result.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure a local query page or a staging write
    /// met; the query's own failures are recorded in the returned capture.
    pub(super) async fn run_entity_query(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        query: Query,
        index: usize,
        context: &ContextSpec,
        materialized_entities: &mut HashSet<Identifier>,
    ) -> Result<QueryCapture, SnapshotError> {
        let mut ledger = CaptureLedger {
            captured: HashSet::new(),
            materialized: materialized_entities,
            staging: Staging::Untouched,
        };
        let detail = self.capture_entity_result(target, snapshot_id, query, index, context, &mut ledger).await?;
        Ok(QueryCapture::of(detail, ledger.staging))
    }

    /// Capture the whole result of one snapshot entity query, recording what
    /// it wrote in `ledger`.
    ///
    /// # Errors
    ///
    /// As [`SnapshotExecutor::run_entity_query`].
    async fn capture_entity_result(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        query: Query,
        index: usize,
        context: &ContextSpec,
        ledger: &mut CaptureLedger<'_>,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        let params = match entity_parameters(query) {
            Ok(params) => params,
            Err(error) => return Ok(failure_detail(classify_parameter_error(&error).into_problem())),
        };
        let mut params = match params.admit(EntityQueryOperation::Query, self.sources.entity_query.filter_admission()) {
            Ok(params) => params,
            Err(error) => return Ok(failure_detail(classify_parameter_error(&error).into_problem())),
        };

        let filters = match self.entity_registration_filters(&params, context).await {
            Ok(filters) => filters,
            Err(error) => return Ok(failure_detail(classify_query_error(&error).into_problem())),
        };
        let budget = ResultBudget::new(self.sources.forwarding.result_limits());
        let (mut pending, forwarding_failure) = match self.walk_sources(&params, context, &filters, &budget).await {
            WalkedSources::Answered(pending) => (pending, None),
            WalkedSources::Unreachable(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot CSR forwarding failed");
                (PendingSources { answers: Vec::new() }, Some(error))
            }
            WalkedSources::TooManyResults(exceeded) => return Ok(failure_detail(classify_result_budget_refusal(&exceeded).into_problem())),
            WalkedSources::TooManyInstances(exceeded) => return Ok(failure_detail(classify_instance_budget_refusal(&exceeded).into_problem())),
        };
        let mut shortfall = SourceShortfall::default();
        shortfall.record(pending.answers.iter().flat_map(|answer| answer.warnings.iter()).inspect(|warning| {
            warn!(snapshot_id = %snapshot_id, query_index = index, warning = %warning, "Snapshot entity query retained a distributed warning");
        }));
        let merge_budget = pending.holds_entities().then_some(&budget);

        // The sources were asked the query as the Snapshot holds it, so the
        // split filters are held back only now, from the local read, and only
        // once a source answered an entity: until then the local entities are
        // the merged ones, as Query Entities reads them.
        let split = !params.is_local() && self.sources.forwarding.split_entities(params.split_entities) == SplitEntities::Split;
        let gathered = split && pending.holds_entities();
        let deferred = gathered.then(|| params.defer_split_filters());
        let query = &self.sources.entity_query;
        let local = LocalRead {
            query: &params,
            context,
            language_targeting: query.language_targeting(),
            filters: if gathered { CandidateFilters::Deferred } else { CandidateFilters::Applied },
        };
        let compiled = match query.compile_query_public(&params, context, local.language_targeting).await {
            Ok(compiled) => compiled,
            Err(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot entity query failed");
                return entity_query_failure(error);
            }
        };
        let capture = EntityCapture {
            target,
            snapshot_id,
            index,
            selection: CaptureSelection {
                deferred,
                list: AnswerList::of(&compiled),
                context,
                dataset_ids: params.dataset_id.as_deref(),
            },
            forwarded: ForwardedCapture::new(self.attribute_nesting),
        };
        // The local entities are walked by keyset over their identifier rather
        // than by offset: entities written or deleted while the capture runs
        // then neither shift a later page nor appear twice (clause 5.5.9.1).
        let mut window = PageWindow {
            limit: self.capture_page_size,
            start: PageStart::Cursor(None),
        };
        // Query Entities holds the merged collection to the entity limit
        // before the split filters decide on it, so every local entity read
        // counts, captured or not.
        let mut local_read = 0_usize;
        loop {
            let states = match self.local_page(local, &window, snapshot_id, index).await? {
                PageOutcome::States(states) => states,
                PageOutcome::Ended(detail) => return Ok(detail),
            };
            let local_page_len = states.len();
            local_read = local_read.saturating_add(local_page_len);
            // clone: the next page starts after the last identifier of this one,
            // while the states themselves move into the capture below.
            let last_read = states.last().map(|state| state.id().clone());
            if matches!(window.start, PageStart::Cursor(None))
                && local_page_len == 0
                && let Some(error) = &forwarding_failure
            {
                return Ok(failure_detail(classify_federation_error(error).into_problem()));
            }
            let claimed = pending.claim(states.iter().map(EntityState::id));
            if let Some(budget) = merge_budget
                && let Err(exceeded) = pending.admit_merged(local_read, budget)
            {
                return Ok(failure_detail(classify_result_budget_refusal(&exceeded).into_problem()));
            }
            let candidates = ForwardedCapture::local_candidates(states, claimed);
            if let Some(failure) = self.capture_candidates(&capture, candidates, ledger).await? {
                return Ok(failure);
            }
            match last_read {
                Some(last) if local_page_len >= self.capture_page_size as usize => window = PageWindow::after(self.capture_page_size, last),
                Some(_) | None => break,
            }
        }

        if let Some(failure) = self.capture_unclaimed(&capture, pending, ledger).await? {
            return Ok(failure);
        }
        Ok(answered_detail(ledger.captured.len(), &shortfall))
    }

    /// Capture the source entities no local page held, merged across
    /// sources, page by page, answering the failure recorded for the query
    /// when one could not be captured.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure selecting or writing a page met.
    async fn capture_unclaimed(
        &self,
        capture: &EntityCapture<'_>,
        pending: PendingSources,
        ledger: &mut CaptureLedger<'_>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        let remaining = EntityQueryService::merge_with_forwarded(Vec::new(), pending.answers);
        let mut remaining = remaining.into_iter().peekable();
        while remaining.peek().is_some() {
            let candidates = remaining
                .by_ref()
                .take(self.capture_page_size as usize)
                .map(|version| CandidateEntity::received(version.state))
                .collect();
            if let Some(failure) = self.capture_candidates(capture, candidates, ledger).await? {
                return Ok(Some(failure));
            }
        }
        Ok(None)
    }

    /// The query's context source filter and geoquery expanded under `context`,
    /// which select the registrations the query reaches exactly as Query
    /// Entities selects them (clauses 4.3.6.1, 4.9 and 5.10.2.4). A query
    /// limited to the local scope reaches none.
    async fn entity_registration_filters(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<ExpandedRegistrationFilters, QueryError> {
        if params.is_local() {
            return Ok(ExpandedRegistrationFilters::default());
        }
        let query = &self.sources.entity_query;
        ExpandedRegistrationFilters::expand(query.engine(), context, params.csf.as_ref(), params.geo.0.as_ref(), query.geo_admission()).await
    }

    /// Walk every matching Context Source to the end of its answer (clause
    /// 5.16.1.4), charging what they send to `budget`.
    async fn walk_sources(
        &self,
        params: &EntityQueryParameters,
        context: &ContextSpec,
        filters: &ExpandedRegistrationFilters,
        budget: &ResultBudget,
    ) -> WalkedSources {
        let forwarding = &self.sources.forwarding;
        let request = ForwardQueryRequest {
            client_query: ClientQuery::EntityQuery(params),
            // A Snapshot materialises its members through the GET query
            // binding (clause 5.5.15).
            delivery: QueryDelivery::Query,
            split: forwarding.split_entities(params.split_entities),
            context,
            triggering: TriggeringRequest::default(),
            csf: filters.csf(),
            geo: filters.geo(),
            entity_map: EntityMapInUse::None,
            budget,
        };
        let forwarded = match forwarding.match_query(&self.sources.live_target, &request).await {
            Ok(matched) => forwarding.forward_query(&self.sources.live_target, request, matched.into_registrations()).await,
            Err(error) => Err(error),
        };
        match forwarded {
            Ok(outcome) => WalkedSources::Answered(PendingSources { answers: outcome.results }),
            Err(QueryForwardingError::TooManyResults(exceeded)) => WalkedSources::TooManyResults(exceeded),
            Err(QueryForwardingError::TooManyInstances(exceeded)) => WalkedSources::TooManyInstances(exceeded),
            Err(QueryForwardingError::Federation(error)) => WalkedSources::Unreachable(error),
        }
    }

    /// Capture the `candidates` of one page the query selects, answering the
    /// failure recorded for the query when they could not be captured.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure selecting or writing the page met.
    async fn capture_candidates(
        &self,
        capture: &EntityCapture<'_>,
        candidates: Vec<CandidateEntity>,
        ledger: &mut CaptureLedger<'_>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        match self.captured_page(candidates, &capture.selection, &capture.forwarded, Utc::now()).await? {
            PageOutcome::States(states) => self.capture_page(capture, states, ledger).await,
            PageOutcome::Ended(detail) => Ok(Some(detail)),
        }
    }

    /// Persist one page of captured states, skipping those an earlier query of
    /// the Snapshot already materialized, and answering the failure recorded
    /// for the query when the page could not be written.
    async fn capture_page(
        &self,
        capture: &EntityCapture<'_>,
        states: Vec<EntityState>,
        ledger: &mut CaptureLedger<'_>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        // clone: the ledger keeps every captured identifier once the states move into the branch write
        let identifiers = states.iter().map(|state| state.id().clone()).collect::<Vec<_>>();
        let fresh = states.into_iter().filter(|state| !ledger.materialized.contains(state.id())).collect::<Vec<_>>();
        if !fresh.is_empty() {
            ledger.staging = Staging::Written;
        }
        let detail = self.persist_current_results(capture.target, capture.snapshot_id, capture.index, fresh).await?;
        if detail.result_status == ExecutionResultStatus::Failure {
            return Ok(Some(detail));
        }
        for identifier in identifiers {
            ledger.materialized.insert(identifier.clone());
            ledger.captured.insert(identifier);
        }
        Ok(None)
    }
}
