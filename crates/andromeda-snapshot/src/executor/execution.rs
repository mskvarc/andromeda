//! Execution of snapshot temporal queries, and the persistence every snapshot
//! query writes its captured results through.
//!
//! A query's own failure is recorded in its execution result (clause 5.2.42).
//! A transient store failure -- a lost deadlock or serialization race, or a
//! saturated pool -- is not the query's: it is returned as an error instead, so
//! the whole preparation restarts rather than freezing a result the load of the
//! moment decided.

use crate::{
    error::{Recurrence, SnapshotError, entity_recurrence, temporal_recurrence},
    executor::{
        SnapshotExecutor,
        execution_failure::ExecutionFailure,
        status::{failure_detail, success_detail},
        translation::temporal_parameters,
    },
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    error::CommitError,
    submission_refusal::SubmissionRefusal,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::error::EntityError;
use andromeda_federation::{
    operation::ForwardingOperation,
    request::{CsrModeFilter, EntityMapInUse, ForwardTemporalQueryRequest, SourcePage, SourcePaging, TriggeringRequest},
    temporal_result::merge_temporal_query_results,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    query::body::Query,
    snapshot::ExecutionResultDetails,
    temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::{PageStart, PageWindow},
    temporal::contributed_entities::ContributedEntities,
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_temporal::{error::TemporalError, service::TemporalQueryResult};
use std::{collections::HashSet, sync::Arc};
use tracing::{error, warn};

impl SnapshotExecutor {
    /// One keyset page of the local temporal entities a snapshot query
    /// captures; no Context Source contributes to the local read.
    ///
    /// The read is boxed: it is the largest future of the capture loop, which
    /// would otherwise carry it inline across every await.
    async fn local_page(&self, params: &TemporalQueryParameters, window: &PageWindow, context: &ContextSpec) -> Result<TemporalQueryResult, TemporalError> {
        Box::pin(self.sources.temporal.query(
            &self.sources.live_target,
            params,
            window,
            CountPolicy::Omit,
            context,
            ContributedEntities::default(),
        ))
        .await
    }

    pub(crate) async fn run_temporal_query(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        query: Query,
        index: usize,
        context: &ContextSpec,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        let params = match temporal_parameters(query) {
            Ok(params) => params,
            Err(error) => return Ok(failure_detail(error.to_string())),
        };
        // The local entities are walked by keyset over their identifier, so
        // entities written while the capture runs neither shift a later page
        // nor appear twice (clause 5.5.9.1); the context sources are paged by
        // the offset their own brokers understand.
        let mut window = PageWindow {
            limit: self.capture_page_size,
            start: PageStart::Cursor(None),
        };
        let mut source_offset = 0_u32;
        let mut captured = HashSet::new();
        loop {
            let result = match self.local_page(&params, &window, context).await {
                Ok(result) => result,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot temporal query failed");
                    return temporal_failure(error, TemporalStep::Query);
                }
            };
            for warning in &result.warnings {
                warn!(snapshot_id = %snapshot_id, query_index = index, entity_id = %warning.entity_id, warning = %warning.reason, "Snapshot temporal query retained usable results with a warning");
            }
            let local_page_len = result.entities.len();
            // clone: the cursor of the next page outlives the entities it was read from
            let last_local = result.entities.last().and_then(TemporalHistory::id).cloned();
            let forwarded = match self
                .sources
                .forwarding
                .forward_temporal_query(
                    &self.sources.live_target,
                    ForwardTemporalQueryRequest {
                        client_query: &params,
                        split: self.sources.forwarding.split_entities(params.split_entities),
                        context,
                        operation: ForwardingOperation::QueryTemporalEntity,
                        triggering: TriggeringRequest::default(),
                        csf_expr: None,
                        mode_filter: CsrModeFilter::All,
                        entity_map: EntityMapInUse::None,
                        paging: SourcePaging::Page(SourcePage {
                            limit: self.capture_page_size,
                            offset: source_offset,
                        }),
                    },
                )
                .await
            {
                Ok(forwarded) => forwarded.results,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot temporal CSR forwarding failed");
                    Vec::new()
                }
            };
            let remote_page_full = forwarded.iter().any(|source| source.entities.len() >= self.capture_page_size as usize);
            let histories = merge_temporal_query_results(result.entities, forwarded, params.timeproperty_or_default());
            let mut identifiers = Vec::with_capacity(histories.len());
            let mut documents = Vec::with_capacity(histories.len());
            for history in histories {
                match history {
                    TemporalHistory::Instances(document) => match document.id().cloned() {
                        Some(id) => {
                            identifiers.push(id);
                            documents.push(document);
                        }
                        None => {
                            warn!(snapshot_id = %snapshot_id, query_index = index, "Temporal snapshot result carries no entity identifier");
                        }
                    },
                    // An aggregate records no instance, so there is no history
                    // of it to capture.
                    TemporalHistory::Aggregates(_) => return Ok(failure_detail("A Snapshot cannot capture an aggregated temporal representation")),
                }
            }
            let detail = self.persist_temporal_results(target, snapshot_id, index, documents).await?;
            if detail.problem_details.is_some() {
                return Ok(detail);
            }
            captured.extend(identifiers);
            if local_page_len < self.capture_page_size as usize && !remote_page_full {
                break;
            }
            // A local walk that ran dry stays where it ended while the sources
            // still page on.
            if let Some(last) = last_local {
                window = PageWindow::after(self.capture_page_size, last);
            }
            source_offset = match source_offset.checked_add(self.capture_page_size) {
                Some(offset) => offset,
                None => return Ok(failure_detail("Snapshot temporal pagination offset overflowed")),
            };
        }
        Ok(success_detail(captured.len()))
    }

    /// Write captured states into the Snapshot's branch as they are, each
    /// through the canonical codec (clause 5.16.1.4).
    pub(super) async fn persist_current_results(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        index: usize,
        states: Vec<EntityState>,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        if states.is_empty() {
            return Ok(success_detail(0));
        }
        let captured = states.len();
        let writes = states
            .into_iter()
            .map(|state| {
                let entity_id = Arc::new(state.id().clone()); // clone: the write owns its validated entity identity beside the state
                Ok(AtomicWrite::builder()
                    .entity_id(entity_id)
                    .entity(EntityMutation::Insert(EntityRecord::from_state(Arc::new(state))?))
                    .build())
            })
            .collect::<Result<Vec<AtomicWrite>, CommitError>>();
        let writes = match writes {
            Ok(writes) => writes,
            Err(error) => {
                error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to read captured Snapshot results");
                return Ok(failure_detail(ExecutionFailure::ReadResults { cause: &error }.to_string()));
            }
        };
        let outcomes = match self.writes.commit(target, writes).await {
            Ok(outcomes) => outcomes,
            Err(error) => {
                error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to materialize current Snapshot results");
                return branch_commit_failure(error);
            }
        };
        // A Snapshot captures the query's results whole, so one captured entity
        // the branch refused fails the query rather than leaving a capture that
        // silently lacks it.
        if let Some(refusal) = outcomes.into_iter().find_map(refusal_of) {
            error!(snapshot_id = %snapshot_id, query_index = index, %refusal, "A captured Snapshot result was refused");
            return branch_commit_failure(CommitError::Submission(refusal));
        }
        Ok(success_detail(captured))
    }

    /// Write captured temporal evolutions into the Snapshot's branch.
    ///
    /// The evolutions arrive in the expanded form, whatever context the query
    /// was spelled in, so what is stored is what clause 5.16.1.4 says to store.
    async fn persist_temporal_results(
        &self,
        target: &ContextTarget,
        snapshot_id: &Identifier,
        index: usize,
        documents: Vec<TemporalDocument>,
    ) -> Result<ExecutionResultDetails, SnapshotError> {
        let captured = documents.len();
        for document in documents {
            if let Err(error) = self.sources.temporal.record_history(target, document).await {
                error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to materialize temporal Snapshot results");
                return temporal_failure(error, TemporalStep::Store);
            }
        }
        Ok(success_detail(captured))
    }
}

/// A failed local entity query: the query's own failure, or a transient store
/// failure the preparation restarts on.
pub(super) fn entity_query_failure(error: EntityError) -> Result<ExecutionResultDetails, SnapshotError> {
    match entity_recurrence(&error) {
        Recurrence::Transient => Err(SnapshotError::from(error)),
        Recurrence::Persistent => Ok(failure_detail(error.to_string())),
    }
}

/// Which temporal operation of a snapshot query failed.
#[derive(Clone, Copy)]
enum TemporalStep {
    /// Reading a page of temporal entities.
    Query,
    /// Writing a captured evolution into the branch.
    Store,
}

/// A failed temporal query or temporal write, `step` naming which: the
/// query's own failure, or a transient store failure the preparation restarts
/// on.
fn temporal_failure(error: TemporalError, step: TemporalStep) -> Result<ExecutionResultDetails, SnapshotError> {
    match temporal_recurrence(&error) {
        Recurrence::Transient => Err(SnapshotError::from(error)),
        Recurrence::Persistent => Ok(failure_detail(match step {
            TemporalStep::Query => error.to_string(),
            TemporalStep::Store => ExecutionFailure::StoreTemporalResults { cause: &error }.to_string(),
        })),
    }
}

/// The refusal a captured entity's write met, if the branch refused it.
fn refusal_of(outcome: WriteOutcome) -> Option<SubmissionRefusal> {
    match outcome {
        WriteOutcome::Refused(refusal) => Some(refusal),
        WriteOutcome::Created
        | WriteOutcome::Updated
        | WriteOutcome::Deleted(_)
        | WriteOutcome::AlreadyExists
        | WriteOutcome::NotFound
        | WriteOutcome::Superseded
        | WriteOutcome::Skipped
        | WriteOutcome::NoEntityChange => None,
    }
}

/// A failed write of captured entities into the staging branch: a transient
/// store failure the preparation restarts on, or the query's own failure.
fn branch_commit_failure(error: CommitError) -> Result<ExecutionResultDetails, SnapshotError> {
    match error {
        CommitError::TransactionContended => Err(SnapshotError::Contended),
        CommitError::StoreSaturated => Err(SnapshotError::StoreSaturated),
        CommitError::StoreUnavailable => Err(SnapshotError::StoreUnavailable),
        failure @ (CommitError::Submission { .. }
        | CommitError::DuplicateEntity { .. }
        | CommitError::Persistence { .. }
        | CommitError::Invariant { .. }
        | CommitError::ReplayLeaseLost { .. }
        | CommitError::UnexpectedOutcomeCount { .. }) => Ok(failure_detail(ExecutionFailure::StoreResults { cause: &failure }.to_string())),
    }
}
