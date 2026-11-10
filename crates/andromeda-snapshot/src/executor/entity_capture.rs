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
//! A local entity is captured as the stored state the query selected, written
//! into the Snapshot's branch as stored. Only an entity a source also answered
//! for, or one only a source holds, goes through the compact merge
//! ([`crate::executor::forwarded_capture`]).
//!
//! The entity limit the sources were walked under holds for the whole capture
//! once a source answered an entity: Query Entities then refuses a merged
//! collection past it (clause 4.12), so each local page is checked against it
//! before it is written. A capture no source answered an entity for pages its
//! local entities unbounded, as Query Entities pages them.

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        captured_state::narrow_captured,
        execution::entity_query_failure,
        execution_failure::ExecutionFailure,
        forwarded_capture::ForwardedCapture,
        pending_sources::PendingSources,
        status::{failure_detail, success_detail},
        translation::entity_parameters,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    port::{retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query::EntityQueryService,
};
use andromeda_federation::{
    entity_result::collect_warnings,
    error::FederationError,
    expanded_registration_filters::ExpandedRegistrationFilters,
    forwarded_query::client_query::ClientQuery,
    instance_budget::InstanceBudgetExceeded,
    outcome::ForwardedEntities,
    query_forwarding_error::QueryForwardingError,
    request::{EntityMapInUse, ForwardQueryRequest, QueryDelivery, TriggeringRequest},
    result_budget::{ResultBudget, ResultBudgetExceeded},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{query::body::Query, snapshot::ExecutionResultDetails};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    error::QueryError,
    page_window::{PageStart, PageWindow},
};
use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingExceeded;
use andromeda_ngsi_ld_request::query::entity::{operation::EntityQueryOperation, parameters::EntityQueryParameters};
use chrono::{DateTime, Utc};
use std::{collections::HashSet, sync::Arc};
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

/// The identifiers a capture has written, for the query and across the
/// Snapshot.
struct CaptureLedger<'a> {
    /// Every entity this query captured.
    captured: HashSet<Identifier>,
    /// Every entity any query of the Snapshot has materialized so far.
    materialized: &'a mut HashSet<Identifier>,
}

impl SnapshotExecutor {
    /// Execute one snapshot entity query and capture its whole result.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure a local query page or a staging write
    /// met; the query's own failures are recorded in the returned details.
    pub(crate) async fn run_entity_query(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        query: Query,
        index: usize,
        context: &ContextSpec,
        materialized_entities: &mut HashSet<Identifier>,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        let params = match entity_parameters(query) {
            Ok(params) => params,
            Err(error) => return Ok(failure_detail(error.to_string())),
        };
        if let Err(error) = params.validate(EntityQueryOperation::Query) {
            return Ok(failure_detail(ExecutionFailure::QueryValidation { cause: &error }.to_string()));
        }

        let filters = match self.entity_registration_filters(&params, context).await {
            Ok(filters) => filters,
            Err(error) => return Ok(failure_detail(error.to_string())),
        };
        let budget = ResultBudget::new(self.sources.forwarding.result_limits());
        let (mut pending, forwarding_failure) = match self.walk_sources(&params, context, &filters, &budget).await {
            WalkedSources::Answered(pending) => (pending, None),
            WalkedSources::Unreachable(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot CSR forwarding failed");
                (PendingSources { answers: Vec::new() }, Some(error))
            }
            WalkedSources::TooManyResults(exceeded) => return Ok(failure_detail(exceeded.to_string())),
            WalkedSources::TooManyInstances(exceeded) => return Ok(failure_detail(exceeded.to_string())),
        };
        for warning in pending.answers.iter().flat_map(|answer| answer.warnings.iter()) {
            warn!(snapshot_id = %snapshot_id, query_index = index, warning = %warning, "Snapshot entity query retained a distributed warning");
        }
        let every_source_failed = !collect_warnings(&pending.answers).is_empty() && !pending.holds_entities();
        let merge_budget = pending.holds_entities().then_some(&budget);

        let capture = ForwardedCapture::new(self.attribute_nesting);
        let mut ledger = CaptureLedger {
            captured: HashSet::new(),
            materialized: materialized_entities,
        };
        // The local entities are walked by keyset over their identifier rather
        // than by offset: entities written or deleted while the capture runs
        // then neither shift a later page nor appear twice (clause 5.5.9.1).
        let mut window = PageWindow {
            limit: self.capture_page_size,
            start: PageStart::Cursor(None),
        };
        loop {
            let mut states = match self.local_page(&params, &window, context, snapshot_id, index).await? {
                LocalPage::States(states) => states,
                LocalPage::Ended(detail) => return Ok(detail),
            };
            let local_page_len = states.len();
            // clone: the next page starts after the last identifier of this one,
            // while the states themselves move into the capture below.
            let last_captured = states.last().map(|state| state.id().clone());
            if matches!(window.start, PageStart::Cursor(None)) && local_page_len == 0 {
                if let Some(error) = &forwarding_failure {
                    return Ok(failure_detail(ExecutionFailure::DistributedExecution { cause: error }.to_string()));
                }
                if every_source_failed {
                    return Ok(failure_detail(
                        "No valid entity result was available because every matching distributed source failed",
                    ));
                }
            }
            let captured_at = Utc::now();
            for state in &mut states {
                narrow_captured(state, captured_at, params.dataset_id.as_deref());
            }
            let claimed = pending.claim(states.iter().map(EntityState::id));
            if let Some(budget) = merge_budget
                && let Err(exceeded) = pending.admit_merged(ledger.captured.len().saturating_add(local_page_len), budget)
            {
                return Ok(failure_detail(exceeded.to_string()));
            }
            let states = match merge_claimed(&capture, states, claimed, captured_at) {
                Ok(states) => states,
                Err(error) => return Ok(failure_detail(error.to_string())),
            };
            if let Some(failure) = self.capture_page(target, snapshot_id, index, states, &mut ledger).await? {
                return Ok(failure);
            }
            match last_captured {
                Some(last) if local_page_len >= self.capture_page_size as usize => window = PageWindow::after(self.capture_page_size, last),
                Some(_) | None => break,
            }
        }

        // The source entities no local page held, merged across sources.
        let remaining = EntityQueryService::merge_with_forwarded(Vec::new(), pending.answers);
        let mut remaining = remaining.into_iter().peekable();
        while remaining.peek().is_some() {
            let page = remaining.by_ref().take(self.capture_page_size as usize).collect();
            let states = match capture.states(page, Utc::now()) {
                Ok(states) => states,
                Err(error) => return Ok(failure_detail(error.to_string())),
            };
            if let Some(failure) = self.capture_page(target, snapshot_id, index, states, &mut ledger).await? {
                return Ok(failure);
            }
        }
        Ok(success_detail(ledger.captured.len()))
    }

    /// One local page of the states `params` selects within `window`, or the
    /// result the query ends with when the page cannot be read whole.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure the page met.
    async fn local_page(
        &self,
        params: &EntityQueryParameters,
        window: &PageWindow,
        context: &ContextSpec,
        snapshot_id: &Identifier,
        index: usize,
    ) -> Result<LocalPage, SnapshotError> {
        let query = &self.sources.entity_query;
        let rows = match query
            .query_states(
                &self.sources.live_target,
                params,
                window,
                CountPolicy::Omit,
                context,
                query.language_targeting(),
            )
            .await
        {
            Ok((rows, _)) => rows,
            Err(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot entity query failed");
                return entity_query_failure(error).map(LocalPage::Ended);
            }
        };
        Ok(match captured_states(rows) {
            Ok(states) => LocalPage::States(states),
            Err(id) => LocalPage::Ended(failure_detail(ExecutionFailure::UnreadableEntity { id: &id }.to_string())),
        })
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
            csf_expr: filters.csf(),
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

    /// Persist one page of captured states, skipping those an earlier query of
    /// the Snapshot already materialized, and answering the failure recorded
    /// for the query when the page could not be written.
    async fn capture_page(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        index: usize,
        states: Vec<EntityState>,
        ledger: &mut CaptureLedger<'_>,
    ) -> Result<Option<ExecutionResultDetails>, SnapshotError> {
        // clone: the ledger keeps every captured identifier once the states move into the branch write
        let identifiers = states.iter().map(|state| state.id().clone()).collect::<Vec<_>>();
        let fresh = states.into_iter().filter(|state| !ledger.materialized.contains(state.id())).collect::<Vec<_>>();
        let detail = self.persist_current_results(target, snapshot_id, index, fresh).await?;
        if detail.problem_details.is_some() {
            return Ok(Some(detail));
        }
        for identifier in identifiers {
            ledger.materialized.insert(identifier.clone());
            ledger.captured.insert(identifier);
        }
        Ok(None)
    }
}

/// What one local page of a capture read.
enum LocalPage {
    /// The page's states, every one read back.
    States(Vec<EntityState>),
    /// The page could not be read whole, and the query ends with this result.
    Ended(ExecutionResultDetails),
}

/// The states of one local page, or the first entity of it that does not read
/// back, without which the capture would not be the query's result.
fn captured_states(rows: Vec<StoredEntry<RetrievedEntity>>) -> Result<Vec<EntityState>, Identifier> {
    rows.into_iter()
        .map(|row| match row {
            // The page statement decoded each state into its own allocation,
            // so taking it out of its `Arc` never copies; the entity-level
            // system times read beside it are not part of a captured entity.
            StoredEntry::Read(row) => Ok(Arc::unwrap_or_clone(row.state)),
            StoredEntry::Unreadable(id) => Err(id),
        })
        .collect()
}

/// One local page's states, the ones a source also answered for merged with
/// what it answered; the others stay as stored.
fn merge_claimed(
    capture: &ForwardedCapture,
    states: Vec<EntityState>,
    claimed: Vec<ForwardedEntities>,
    captured_at: DateTime<Utc>,
) -> Result<Vec<EntityState>, AttributeNestingExceeded> {
    if claimed.is_empty() {
        return Ok(states);
    }
    let (answered, unanswered): (Vec<EntityState>, Vec<EntityState>) = {
        let answered_ids: HashSet<&Identifier> = claimed
            .iter()
            .flat_map(|answer| answer.entities.iter().map(|received| received.state.id()))
            .collect();
        states.into_iter().partition(|state| answered_ids.contains(state.id()))
    };
    let mut merged = capture.merged(answered, claimed, captured_at)?;
    merged.extend(unanswered);
    Ok(merged)
}
