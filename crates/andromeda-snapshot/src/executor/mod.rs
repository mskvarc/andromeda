//! Snapshot query execution for NGSI-LD clause 5.16.1.4.

mod attempts;
/// Where a snapshot capture reads the context it freezes.
pub mod capture_sources;
/// What a captured stored state keeps of what it holds.
mod captured_state;
mod drain;
/// Capturing the whole result of one snapshot entity query.
mod entity_capture;
mod execution;
/// Why a Snapshot query or its preparation is recorded as failed.
mod execution_failure;
/// Turning the entities a capture merged with its sources into states.
mod forwarded_capture;
/// Reading the instances of a captured temporal answer to the end of its window.
mod instance_walk;
mod lease_renewal;
/// Reading what the Context Sources answer a captured temporal query to the end of its window.
mod source_instance_walk;
mod status;
/// Capturing the whole answer of one snapshot temporal query.
mod temporal_capture;
/// Recording a failed temporal step of a snapshot query.
mod temporal_failure;
mod translation;

use crate::{
    error::SnapshotError,
    executor::{
        attempts::{PreparationClaim, PreparationEnd, QueryCounts},
        capture_sources::SnapshotCaptureSources,
        drain::DrainWorker,
        lease_renewal::LeaseRenewal,
    },
    snapshot_invariant::SnapshotInvariant,
    status::validate_preparing_transition,
    store::{SnapshotPreparationJob, SnapshotStore},
};
use andromeda_context_commit::{committer::ContextCommitter, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::snapshot::Snapshot;
use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
use chrono::Utc;
use futures::future::BoxFuture;
use std::{collections::HashSet, sync::Arc, time::Duration};
use tracing::{debug, info, warn};

/// Executes a snapshot's typed queries and persists their frozen results.
pub struct SnapshotExecutor {
    sources: SnapshotCaptureSources,
    snapshot_repo: Arc<dyn SnapshotStore>,
    writes: Arc<dyn ContextCommitter>,
    capture_page_size: u32,
    /// The deepest nesting an attribute a Context Source answered may have to
    /// be captured.
    attribute_nesting: AttributeNestingLimit,
}

impl SnapshotExecutor {
    /// Creates a snapshot executor wired to the query services.
    pub fn new(
        sources: SnapshotCaptureSources,
        snapshot_repo: Arc<dyn SnapshotStore>,
        writes: Arc<dyn ContextCommitter>,
        capture_page_size: usize,
        attribute_nesting: AttributeNestingLimit,
    ) -> SnapshotExecutor {
        SnapshotExecutor {
            sources,
            snapshot_repo,
            writes,
            capture_page_size: u32::try_from(capture_page_size).unwrap_or(u32::MAX),
            attribute_nesting,
        }
    }

    /// Execute all queries for a snapshot and record its final status.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Internal`] when the snapshot owns no durable
    /// staging context branch, [`SnapshotError::Validation`] when the aggregated
    /// execution status is not a permitted transition out of `preparing`, a
    /// transient store failure a query or a staging write met, and the store's
    /// own failure when promoting the capture does not complete.
    pub async fn execute(&self, snapshot: &Snapshot, query_context: &JsonLiteral, claimed_by: LeaseHolder) -> Result<(), SnapshotError> {
        info!(snapshot_id = %snapshot.id, "Starting snapshot query execution");
        let target = self
            .snapshot_repo
            .staging_target(&snapshot.id)
            .await?
            .map(ContextTarget::Snapshot)
            .ok_or_else(|| SnapshotInvariant::NoStagingTarget {
                snapshot: snapshot.id.clone(), // clone: the error owns the Snapshot it names.
            })?;
        let mut aggregate = status::ExecutionAggregate::default();
        let context = persisted_query_context(query_context);

        let mut materialized_entities = HashSet::new();
        for (index, query) in snapshot.snapshot_queries.iter().flatten().enumerate() {
            let detail = self
                // clone: a restarted preparation translates every stored query again.
                .run_entity_query(&target, &snapshot.id, query.clone(), index, &context, &mut materialized_entities)
                .await?;
            aggregate.push_entity(detail);
        }
        for (index, query) in snapshot.snapshot_temporal_queries.iter().flatten().enumerate() {
            // clone: a restarted preparation translates every stored query again.
            let detail = self.run_temporal_query(&target, &snapshot.id, query.clone(), index, &context).await?;
            aggregate.push_temporal(detail);
        }

        let final_status = aggregate.final_status();
        debug!(snapshot_id = %snapshot.id, status = %final_status, "Snapshot query execution completed");
        validate_preparing_transition(final_status)?;

        let (query_details, temporal_query_details) = aggregate.detail_slices();
        self.snapshot_repo
            .promote_capture(&snapshot.id, claimed_by, final_status, query_details, temporal_query_details)
            .await
    }
}

/// The context a Snapshot's persisted queries run under.
///
/// The persisted context is the one the Create Snapshot request resolved, which
/// already ends with the v1.9 core context. It is read like any request context
/// all the same, so a core reference it holds of another release is processed
/// as the v1.9 core context, last (clause 4.4), rather than as the release it
/// names.
fn persisted_query_context(query_context: &JsonLiteral) -> ContextSpec {
    // clone: the spec owns the body context it processes under, and a
    // restarted preparation reads the persisted literal again.
    ContextSpec::from_body(query_context.clone())
}

/// Outcome of one bounded preparation sweep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotPreparationOutcome {
    /// No snapshot is waiting for preparation.
    Idle,
    /// One or more preparing snapshots were completed.
    Prepared {
        /// Number completed during this bounded pass.
        count: usize,
    },
}

/// Bounded operation that executes the next persisted preparing snapshot.
pub struct SnapshotPreparation {
    store: Arc<dyn SnapshotStore>,
    executor: SnapshotExecutor,
    max_per_pass: usize,
    worker_id: LeaseHolder,
    lease_duration: Duration,
    tenant_count_quota: Option<usize>,
    tenant_byte_quota: Option<u64>,
}

/// Bounded execution and resource policies for Snapshot preparation.
#[derive(Clone, Copy, Debug)]
pub struct SnapshotPreparationConfig {
    /// Maximum source page size used during capture.
    pub capture_page_size: usize,
    /// Maximum preparations executed concurrently by one worker pass.
    pub max_per_pass: usize,
    /// Renewable durable-claim lifetime.
    pub lease_duration: Duration,
    /// Optional per-tenant Snapshot count quota.
    pub tenant_count_quota: Option<usize>,
    /// Optional per-tenant Snapshot byte quota.
    pub tenant_byte_quota: Option<u64>,
    /// The deepest nesting an attribute a Context Source answered may have to
    /// be captured; the one every write is held to.
    pub attribute_nesting: AttributeNestingLimit,
}

impl SnapshotPreparation {
    /// Construct preparation from focused policy ports and query operations.
    pub fn new(
        sources: SnapshotCaptureSources,
        store: Arc<dyn SnapshotStore>,
        writes: Arc<dyn ContextCommitter>,
        config: SnapshotPreparationConfig,
    ) -> SnapshotPreparation {
        SnapshotPreparation {
            executor: SnapshotExecutor::new(
                sources,
                Arc::clone(&store), // clone: the preparation claims the jobs its executor promotes through the same store.
                writes,
                config.capture_page_size,
                config.attribute_nesting,
            ),
            store,
            max_per_pass: config.max_per_pass.max(1),
            worker_id: LeaseHolder::mint(),
            lease_duration: config.lease_duration,
            tenant_count_quota: config.tenant_count_quota,
            tenant_byte_quota: config.tenant_byte_quota,
        }
    }

    /// Execute every claimable preparation, in rounds of at most the
    /// configured concurrency.
    ///
    /// Each claimed preparation ends in a terminal status within this pass: a
    /// transient failure restarts it promptly under the same claim, and one
    /// that persists records `failure` (clause 5.16.1.4). Only a preparation
    /// whose terminal status cannot be recorded at all gives its claim up, so
    /// the next pass of any worker takes it without waiting out the lease. A
    /// clone whose source this pass ended is claimed in a later round of the
    /// same pass (clause 5.16.2.4).
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Internal`] when the configured lease duration
    /// cannot be turned into a wall-clock deadline, and the store's own failure
    /// when claiming a preparing snapshot does not complete before any
    /// preparation was moved on.
    pub async fn execute(&self) -> Result<SnapshotPreparationOutcome, SnapshotError> {
        let worker = DrainWorker {
            store: self.store.as_ref(),
            worker: self.worker_id,
            lease: self.lease_duration,
            max_per_round: self.max_per_pass,
        };
        // Boxed: one capture drives the whole query pipeline, so its future is far
        // too large to hold inline for every buffered job of a round.
        let count = worker.drain(|job| Box::pin(self.prepare(job))).await?;
        if count == 0 {
            Ok(SnapshotPreparationOutcome::Idle)
        } else {
            Ok(SnapshotPreparationOutcome::Prepared { count })
        }
    }

    /// Drive one claimed job to its end, then enforce the tenant quotas a
    /// terminal status may have pushed past; `None` when its terminal status
    /// could not be recorded and its claim was given back.
    async fn prepare(&self, job: SnapshotPreparationJob) -> Option<PreparationEnd> {
        let identifier = &job.snapshot.id;
        match self.execute_with_lease_renewal(&job).await {
            Ok(end @ (PreparationEnd::Completed | PreparationEnd::Failed)) => {
                if let Err(error) = self.store.enforce_quotas(self.tenant_count_quota, self.tenant_byte_quota).await {
                    warn!(snapshot_id = %identifier, %error, "Snapshot quota enforcement after preparation failed");
                }
                Some(end)
            }
            Ok(PreparationEnd::ClaimLost) => {
                info!(snapshot_id = %identifier, "Snapshot preparation claim was taken over or the Snapshot is gone; leaving its outcome to its holder");
                Some(PreparationEnd::ClaimLost)
            }
            Err(error) => {
                warn!(snapshot_id = %identifier, %error, "Snapshot preparation could not record a terminal status; releasing its claim");
                if let Err(error) = self.store.release_preparation(identifier, self.worker_id).await {
                    warn!(snapshot_id = %identifier, %error, "Snapshot preparation claim could not be released; it is reclaimable once its lease expires");
                }
                None
            }
        }
    }

    /// Execute one claimed job while periodically extending this replica's durable claim.
    async fn execute_with_lease_renewal(&self, job: &SnapshotPreparationJob) -> Result<PreparationEnd, SnapshotError> {
        let claim = PreparationClaim {
            store: self.store.as_ref(),
            id: &job.snapshot.id,
            worker: self.worker_id,
            lease: self.lease_duration,
            queries: QueryCounts::of(job),
        };
        let renewal = LeaseRenewal {
            store: self.store.as_ref(),
            id: &job.snapshot.id,
            worker: self.worker_id,
            lease: self.lease_duration,
        };
        renewal.drive(claim.run(|| self.attempt(job))).await
    }

    /// One attempt at a claimed job: clone its source into it, or capture its
    /// queries. Boxed: the capture drives the whole query pipeline, so its
    /// future is far too large to hold inline in the retry loop.
    fn attempt<'a>(&'a self, job: &'a SnapshotPreparationJob) -> BoxFuture<'a, Result<(), SnapshotError>> {
        match &job.clone_source_id {
            Some(source_id) => self.store.promote_clone(source_id, &job.snapshot.id, self.worker_id),
            None => Box::pin(self.executor.execute(&job.snapshot, &job.query_context, self.worker_id)),
        }
    }
}

/// A checked wall-clock deadline `lease` from now.
fn lease_deadline(lease: Duration) -> Result<chrono::DateTime<Utc>, SnapshotError> {
    let lease = chrono::Duration::from_std(lease).map_err(SnapshotInvariant::LeaseOutOfRange)?;
    Utc::now()
        .checked_add_signed(lease)
        .ok_or_else(|| SnapshotInvariant::LeaseDeadlineOverflow.into())
}

#[cfg(test)]
mod tests {
    use crate::executor::persisted_query_context;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use serde_json::{Value, json};

    /// The context a persisted query context is processed under, as JSON.
    fn resolved(persisted: &Value) -> Value {
        let literal = JsonLiteral::admit(&persisted.to_string()).expect("an admissible context");
        let context = persisted_query_context(&literal);
        serde_json::from_str(context.resolved_context().text()).expect("JSON")
    }

    /// Clause 4.4: a persisted context naming an older core context is
    /// processed as the v1.9 core context, last, like any request context.
    #[test]
    fn a_persisted_older_core_reference_is_processed_as_the_current_core_context_last() {
        let persisted = json!([
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld",
            "https://example.com/user.jsonld"
        ]);

        pretty_assertions::assert_eq!(
            resolved(&persisted),
            json!(["https://example.com/user.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()])
        );
    }

    /// The context the Create Snapshot request persisted already ends with the
    /// v1.9 core context, and is processed unchanged.
    #[test]
    fn a_persisted_context_ending_with_the_current_core_context_is_processed_unchanged() {
        let persisted = json!(["https://example.com/user.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()]);

        pretty_assertions::assert_eq!(resolved(&persisted), persisted);
    }
}
