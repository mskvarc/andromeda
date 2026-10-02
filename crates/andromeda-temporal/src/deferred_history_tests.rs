use crate::{
    append_intent::{KeyedTemporalAppendIntent, TemporalAppendEntry, TemporalAppendIntent, TemporalIntentKey, TemporalObservationIdentity},
    error::TemporalError,
    intent_drain::{TemporalIntentClaim, TemporalIntentDrain},
    intent_retry_policy::TemporalIntentRetryPolicy,
    observed_selection::ObservedSelection,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    pending_overlay::PendingTemporalHistoryOverlay,
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            RecordedAttributeShape,
            TemporalHistoryObservation,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        mutation_barrier::TemporalMutationBarrier,
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_history_sink::TemporalHistorySink,
        temporal_intent_queue::{ClaimedTemporalAppendIntent, TemporalIntentBatchSize, TemporalIntentQueue, TemporalIntentRetry},
    },
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_operation_unsupported::TemporalOperationUnsupported,
};
use andromeda_context_commit::{
    failure_summary::FAILURE_SUMMARY_MAX_CHARACTERS,
    lease_acknowledgement::LeaseAcknowledgement,
    lease_holder::LeaseHolder,
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    temporal_format::TemporalFormat,
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    temporal::{compiled::CompiledTemporalQuery, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use indexmap::IndexMap;
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
    slice,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

#[derive(Default)]
struct RecordingQueueState {
    pending: Vec<ClaimedTemporalAppendIntent>,
    acknowledged: Vec<WriteId>,
    retries: Vec<TemporalIntentRetry>,
    /// When set, every acknowledgement and retry reports a lapsed lease.
    lease_lapsed: bool,
}

#[derive(Default)]
struct RecordingQueue {
    state: Mutex<RecordingQueueState>,
}

impl RecordingQueue {
    async fn insert(&self, pending: KeyedTemporalAppendIntent) {
        self.state.lock().await.pending.push(ClaimedTemporalAppendIntent { pending, attempts: 0 });
    }

    async fn pending_count(&self) -> usize {
        self.state.lock().await.pending.len()
    }

    async fn acknowledged_count(&self) -> usize {
        self.state.lock().await.acknowledged.len()
    }

    async fn retry_count(&self) -> usize {
        self.state.lock().await.retries.len()
    }

    async fn retries(&self) -> Vec<TemporalIntentRetry> {
        self.state.lock().await.retries.clone()
    }

    async fn lapse_leases(&self) {
        self.state.lock().await.lease_lapsed = true;
    }
}

#[async_trait]
impl PendingTemporalIntentReader for RecordingQueue {
    async fn pending_for_entity(&self, entity_id: &Identifier) -> Result<Vec<KeyedTemporalAppendIntent>, TemporalError> {
        Ok(self
            .state
            .lock()
            .await
            .pending
            .iter()
            .filter(|claim| claim.pending.intent.entity_id == *entity_id)
            .map(|claim| claim.pending.clone())
            .collect())
    }

    async fn pending_intents(&self) -> Result<Vec<KeyedTemporalAppendIntent>, TemporalError> {
        Ok(self.state.lock().await.pending.iter().map(|claim| claim.pending.clone()).collect())
    }
}

#[async_trait]
impl TemporalIntentQueue for RecordingQueue {
    async fn claim(
        &self,
        _owner: LeaseHolder,
        limit: TemporalIntentBatchSize,
        _lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        let limit = usize::try_from(limit.get()).unwrap_or(usize::MAX);
        Ok(self.state.lock().await.pending.iter().take(limit).cloned().collect())
    }

    async fn claim_for_entity(
        &self,
        _owner: LeaseHolder,
        entity_id: &Identifier,
        limit: TemporalIntentBatchSize,
        _lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        let limit = usize::try_from(limit.get()).unwrap_or(usize::MAX);
        Ok(self
            .state
            .lock()
            .await
            .pending
            .iter()
            .filter(|claim| claim.pending.intent.entity_id == *entity_id)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn acknowledge(&self, _owner: LeaseHolder, intent: &KeyedTemporalAppendIntent) -> Result<LeaseAcknowledgement, TemporalError> {
        let mut state = self.state.lock().await;
        if state.lease_lapsed {
            return Ok(LeaseAcknowledgement::Lost);
        }
        state
            .pending
            .retain(|claim| claim.pending.intent.record.write_id() != intent.intent.record.write_id());
        state.acknowledged.push(intent.intent.record.write_id());
        Ok(LeaseAcknowledgement::Applied)
    }

    async fn retry(&self, _owner: LeaseHolder, intent: &KeyedTemporalAppendIntent, retry: TemporalIntentRetry) -> Result<LeaseAcknowledgement, TemporalError> {
        let mut state = self.state.lock().await;
        if state.lease_lapsed {
            return Ok(LeaseAcknowledgement::Lost);
        }
        if let Some(claim) = state
            .pending
            .iter_mut()
            .find(|claim| claim.pending.intent.record.write_id() == intent.intent.record.write_id())
        {
            claim.attempts = claim.attempts.saturating_add(1);
        }
        state.retries.push(retry);
        Ok(LeaseAcknowledgement::Applied)
    }
}

/// How the recording sink refuses an append while it is told to.
#[derive(Clone, Copy)]
enum SinkRejection {
    /// The history store is unreachable.
    StoreUnavailable,
    /// A refusal whose rendering runs past the failure-summary bound: three
    /// identifiers near the identifier limit in one not-found reason.
    LongDiagnostic,
}

impl SinkRejection {
    fn error(self) -> TemporalError {
        match self {
            SinkRejection::StoreUnavailable => TemporalError::from(PersistenceFailure::Unavailable),
            SinkRejection::LongDiagnostic => {
                let long = |prefix: &str| format!("{prefix}{}", "x".repeat(1000));
                TemporalError::NotFound(TemporalNotFoundReason::AttributeDatasetOnEntity {
                    attribute: long("https://example.org/").parse().expect("a valid long attribute name"),
                    dataset_id: Box::new(long("urn:ngsi-ld:Dataset:").parse().expect("a valid long dataset identifier")),
                    entity: long("urn:ngsi-ld:Entity:").parse().expect("a valid long entity identifier"),
                })
            }
        }
    }
}

#[derive(Default)]
struct RecordingSinkState {
    observations: HashMap<WriteId, (Identifier, Vec<TemporalHistoryObservation>)>,
    /// The failure every append reports while set.
    reject: Option<SinkRejection>,
}

#[derive(Default)]
struct RecordingSink {
    state: Mutex<RecordingSinkState>,
}

impl RecordingSink {
    async fn reject(&self) {
        self.reject_with(SinkRejection::StoreUnavailable).await;
    }

    async fn reject_with(&self, rejection: SinkRejection) {
        self.state.lock().await.reject = Some(rejection);
    }

    async fn logical_effect_count(&self) -> usize {
        self.state.lock().await.observations.len()
    }
}

#[async_trait]
impl TemporalHistorySink for RecordingSink {
    async fn append(&self, _target: &ContextTarget, intent: &TemporalAppendIntent) -> Result<(), TemporalError> {
        let mut state = self.state.lock().await;
        if let Some(rejection) = state.reject {
            return Err(rejection.error());
        }
        let observations = intent
            .history_entries()
            .filter_map(|(identity, entry)| match entry {
                TemporalAppendEntry::Observation { row, .. } => Some(TemporalHistoryObservation {
                    row: row.clone(), // clone: the recording sink owns the accepted observation.
                    identity: Some(identity),
                }),
                TemporalAppendEntry::Tombstone {
                    attribute_name,
                    dataset_id,
                    attribute_type,
                    instance,
                } => Some(TemporalHistoryObservation {
                    row: TemporalHistoryRow {
                        attribute_name: attribute_name.clone(), // clone: the recording sink owns the tombstone identity.
                        attribute_type: *attribute_type,
                        dataset_id: dataset_id.clone(), // clone: the recording sink owns the dataset identity.
                        value: TemporalValue::Tombstone {
                            deleted_at: intent.record.recorded_at(),
                        },
                        observed_at: None,
                        recorded_at: intent.record.recorded_at(),
                        created_at: None,
                        modified_at: None,
                        instance: instance.clone(), // clone: the recording sink owns the instance identity.
                        metadata: None,
                        partition_total: None,
                    },
                    identity: Some(identity),
                }),
                TemporalAppendEntry::Scope { .. } => None,
            })
            .collect();
        state.observations.insert(intent.record.write_id(), (intent.entity_id.clone(), observations)); // clone: the recording sink owns the entity identity.
        Ok(())
    }
}

#[async_trait]
impl TemporalHistoryReader for RecordingSink {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            aggregation: AggregationPushdown::None,
            history_visibility: HistoryVisibility::Eventual {
                maximum_lag: Duration::from_secs(1),
            },
        }
    }

    async fn entity_exists(&self, _target: &ContextTarget, entity_id: &Identifier) -> Result<bool, TemporalError> {
        Ok(self.state.lock().await.observations.values().any(|(stored_id, _)| stored_id == entity_id))
    }

    async fn entity_types(&self, _target: &ContextTarget, entity_ids: &[Identifier]) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError> {
        let state = self.state.lock().await;
        Ok(entity_ids
            .iter()
            .filter(|entity_id| state.observations.values().any(|(stored_id, _)| stored_id == *entity_id))
            .map(|entity_id| (entity_id.clone(), Vec::new())) // clone: the index owns its validated entity URI.
            .collect())
    }

    async fn query_entity_ids(&self, _target: &ContextTarget, _compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
        let state = self.state.lock().await;
        let mut identifiers = state
            .observations
            .values()
            .map(|(entity_id, _)| entity_id.clone()) // clone: the reader outcome owns its identifier.
            .collect::<Vec<_>>();
        identifiers.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        identifiers.dedup();
        Ok(TemporalEntityIdOutcome {
            candidates: identifiers
                .into_iter()
                .map(|entity_id| TemporalEntityCandidate { entity_id, selection: None })
                .collect(),
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Deferred,
        })
    }

    async fn query_entity_history(
        &self,
        _target: &ContextTarget,
        entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        let observations = self
            .state
            .lock()
            .await
            .observations
            .values()
            .filter(|(stored_id, _)| stored_id == entity_id)
            .flat_map(|(_, observations)| observations.clone()) // clone: the reader outcome owns rows retained by the recording sink.
            .collect();
        Ok(TemporalHistoryOutcome { observations })
    }

    async fn query_entity_aggregation(
        &self,
        _target: &ContextTarget,
        _entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        Err(TemporalError::OperationNotSupported(TemporalOperationUnsupported::RecordingSinkNoAggregation))
    }
    async fn recorded_attribute_shapes(
        &self,
        _target: &ContextTarget,
        _attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        Ok(HashMap::new())
    }
}

fn fixture() -> (KeyedTemporalAppendIntent, Identifier) {
    let entity_id = "urn:ngsi-ld:Vehicle:deferred".parse::<Identifier>().expect("valid entity ID");
    let record = WriteRecord::mint(instant());
    let row = TemporalHistoryRow {
        attribute_name: "https://example.com/speed".parse().expect("valid attribute name"),
        attribute_type: AttributeType::Property,
        dataset_id: None,
        value: TemporalValue::Number(JsonNumber::from(42)),
        observed_at: Some(instant()),
        recorded_at: instant(),
        created_at: Some(instant()),
        modified_at: Some(instant()),
        instance: InstanceIdentity::generate(),
        metadata: None,
        partition_total: None,
    };
    let pending = KeyedTemporalAppendIntent {
        key: TemporalIntentKey::new(1).expect("positive key"),
        intent: TemporalAppendIntent {
            record,
            base_ordinal: WriteOrdinal::at(0),
            entity_id: entity_id.clone(), // clone: fixture returns the identifier separately from the intent.
            entity_types: Vec::new(),
            entries: vec![TemporalAppendEntry::Observation { row }],
            selection_value: Some(ObservedSelection::Written(Arc::new(EntityDocument::new(
                Some(entity_id.clone()), // clone: the observed document names the entity the fixture returns too.
                Vec::new(),
                None,
                None,
                IndexMap::new(),
            )))),
        },
    };
    (pending, entity_id)
}

fn instant() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant")
}

fn live_target() -> ContextTarget {
    ContextTarget::Live
}

fn query() -> CompiledTemporalQuery {
    CompiledTemporalQuery::builder()
        .entities(EntitySelection::builder().build())
        .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
        .instances(InstanceSelection::builder().build())
        .representation(
            Representation::builder()
                .format(TemporalFormat::Normalized)
                .sys_attrs(SysAttrsInclusion::Strip)
                .build(),
        )
        .build()
}

/// The retry schedule the drains under test use: two seconds a step, capped
/// at five, stuck from the third completed attempt.
fn retry_policy() -> TemporalIntentRetryPolicy {
    TemporalIntentRetryPolicy::new(Duration::from_secs(2), Duration::from_secs(5), NonZeroU32::new(3).expect("non-zero threshold"))
        .expect("the ceiling is above the base delay")
}

fn drain(queue: Arc<RecordingQueue>, sink: Arc<RecordingSink>) -> TemporalIntentDrain {
    TemporalIntentDrain::new(
        Arc::clone(&queue) as Arc<dyn TemporalIntentQueue>,
        queue as Arc<dyn PendingTemporalIntentReader>,
        sink,
        TemporalIntentClaim {
            owner: LeaseHolder::mint(),
            batch_size: TemporalIntentBatchSize::new(NonZeroU32::new(8).expect("non-zero batch")),
            lease_duration: Duration::from_secs(30),
            retry_policy: retry_policy(),
        },
    )
}

async fn overlay_rows(queue: Arc<RecordingQueue>, sink: Arc<RecordingSink>, entity_id: &Identifier) -> Vec<TemporalHistoryObservation> {
    PendingTemporalHistoryOverlay::new(sink, queue)
        .query_entity_history(&live_target(), entity_id, &query())
        .await
        .expect("overlay read")
        .observations
}

#[tokio::test]
async fn committed_pending_intent_is_immediately_visible_once() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, entity_id) = fixture();
    queue.insert(pending).await;
    let overlay = PendingTemporalHistoryOverlay::new(
        Arc::clone(&sink) as Arc<dyn TemporalHistoryReader>,
        Arc::clone(&queue) as Arc<dyn PendingTemporalIntentReader>,
    );
    assert!(overlay.entity_exists(&live_target(), &entity_id).await.expect("logical entity existence"));
    let rows = overlay_rows(queue, sink, &entity_id).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].identity.is_some());
}

#[tokio::test]
async fn successful_drain_moves_visibility_from_overlay_to_sink_without_changing_rows() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, entity_id) = fixture();
    queue.insert(pending).await;
    let before = overlay_rows(Arc::clone(&queue), Arc::clone(&sink), &entity_id).await;
    drain(Arc::clone(&queue), Arc::clone(&sink)).execute(instant()).await.expect("drain succeeds");
    let after = overlay_rows(Arc::clone(&queue), Arc::clone(&sink), &entity_id).await;
    pretty_assertions::assert_eq!(before, after);
    assert_eq!(queue.pending_count().await, 0);
    assert_eq!(queue.acknowledged_count().await, 1);
}

#[tokio::test]
async fn mutation_barrier_delivers_the_target_entity_before_returning() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, entity_id) = fixture();
    queue.insert(pending).await;
    drain(Arc::clone(&queue), Arc::clone(&sink))
        .synchronize(&live_target(), &entity_id)
        .await
        .expect("entity synchronization succeeds");
    assert_eq!(queue.pending_count().await, 0);
    assert_eq!(sink.logical_effect_count().await, 1);
}

#[tokio::test]
async fn sink_acceptance_before_acknowledgement_deduplicates_by_intent_identity() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, entity_id) = fixture();
    queue.insert(pending.clone()).await;
    sink.append(&live_target(), &pending.intent).await.expect("sink accepts intent");
    let rows = overlay_rows(queue, sink, &entity_id).await;
    assert_eq!(rows.len(), 1);
    pretty_assertions::assert_eq!(
        rows[0].identity,
        Some(TemporalObservationIdentity {
            write_id: pending.intent.record.write_id(),
            ordinal: WriteOrdinal::at(0)
        })
    );
}

#[tokio::test]
async fn restart_after_anchor_commit_retains_overlay_and_allows_delivery() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, entity_id) = fixture();
    queue.insert(pending).await;
    assert_eq!(overlay_rows(Arc::clone(&queue), Arc::clone(&sink), &entity_id).await.len(), 1);
    drain(Arc::clone(&queue), Arc::clone(&sink))
        .execute(instant())
        .await
        .expect("restarted drain succeeds");
    assert_eq!(sink.logical_effect_count().await, 1);
    assert_eq!(queue.pending_count().await, 0);
}

#[tokio::test]
async fn lost_acknowledgement_replays_successfully_with_one_logical_sink_effect() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, _entity_id) = fixture();
    queue.insert(pending.clone()).await;
    sink.append(&live_target(), &pending.intent).await.expect("first sink effect succeeds");
    drain(Arc::clone(&queue), Arc::clone(&sink)).execute(instant()).await.expect("replay succeeds");
    assert_eq!(sink.logical_effect_count().await, 1);
    assert_eq!(queue.acknowledged_count().await, 1);
}

#[tokio::test]
async fn sink_failure_records_retry_and_keeps_pending_overlay_visible() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, entity_id) = fixture();
    queue.insert(pending).await;
    sink.reject().await;
    let outcome = drain(Arc::clone(&queue), Arc::clone(&sink))
        .execute(instant())
        .await
        .expect("failure is retained per item");
    assert_eq!(outcome.retried, 1);
    assert_eq!(outcome.lease_lost, 0);
    assert_eq!(queue.retry_count().await, 1);
    assert_eq!(queue.pending_count().await, 1);
    assert_eq!(queue.acknowledged_count().await, 0);
    assert_eq!(overlay_rows(queue, sink, &entity_id).await.len(), 1);
}

/// The retry delay grows a base step per completed attempt and stops at the
/// policy ceiling, so an intent that keeps failing is retried at a bounded
/// cadence instead of backing off without limit.
#[tokio::test]
async fn repeated_sink_failures_back_off_linearly_up_to_the_configured_ceiling() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, _entity_id) = fixture();
    queue.insert(pending).await;
    sink.reject().await;
    let drain = drain(Arc::clone(&queue), Arc::clone(&sink));
    for _ in 0..4 {
        drain.execute(instant()).await.expect("failure is retained per item");
    }
    let delays: Vec<TimeDelta> = queue.retries().await.iter().map(|retry| retry.eligible_at - instant()).collect();
    assert_eq!(
        delays,
        vec![TimeDelta::seconds(2), TimeDelta::seconds(4), TimeDelta::seconds(5), TimeDelta::seconds(5)],
        "two seconds a step, clamped at the five-second ceiling"
    );
}

/// An acknowledgement or retry whose lease has lapsed is counted as lost, not
/// as delivered or retried, and is left to the worker that now holds the row.
#[tokio::test]
async fn a_lapsed_lease_is_reported_as_lost_rather_than_as_an_outcome() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, _entity_id) = fixture();
    queue.insert(pending).await;
    queue.lapse_leases().await;
    let outcome = drain(Arc::clone(&queue), Arc::clone(&sink))
        .execute(instant())
        .await
        .expect("a lapsed lease is an outcome, not a failure");
    assert_eq!(outcome.acknowledged, 0);
    assert_eq!(outcome.retried, 0);
    assert_eq!(outcome.lease_lost, 1);
    assert_eq!(queue.acknowledged_count().await, 0, "the row is left to its new owner");
    assert_eq!(queue.pending_count().await, 1);
}

/// The sink's failure summary is cut to what the queue row can hold.
#[tokio::test]
async fn a_long_sink_failure_summary_is_cut_to_the_column_limit() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, _entity_id) = fixture();
    queue.insert(pending).await;
    sink.reject_with(SinkRejection::LongDiagnostic).await;
    drain(Arc::clone(&queue), Arc::clone(&sink))
        .execute(instant())
        .await
        .expect("failure is retained per item");
    let summaries: Vec<usize> = queue.retries().await.iter().map(|retry| retry.summary.chars().count()).collect();
    assert_eq!(summaries, vec![usize::try_from(FAILURE_SUMMARY_MAX_CHARACTERS).expect("fits usize")]);
}

fn snapshot_target() -> ContextTarget {
    ContextTarget::Snapshot(SnapshotTarget::published(
        SnapshotKey::try_from(7).expect("a positive key"),
        "urn:ngsi-ld:Snapshot:deferred".parse().expect("a valid snapshot URI"),
    ))
}

/// Only the live branch defers its history, so an intent the queue holds is
/// the live branch's: a Snapshot read neither sees its rows, nor counts its
/// entity as existing, nor lists it among the matching entities.
#[tokio::test]
async fn a_snapshot_read_does_not_overlay_the_live_branchs_pending_intents() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, entity_id) = fixture();
    queue.insert(pending).await;
    let overlay = PendingTemporalHistoryOverlay::new(
        Arc::clone(&sink) as Arc<dyn TemporalHistoryReader>,
        Arc::clone(&queue) as Arc<dyn PendingTemporalIntentReader>,
    );
    let snapshot = snapshot_target();

    let rows = overlay
        .query_entity_history(&snapshot, &entity_id, &query())
        .await
        .expect("snapshot history read")
        .observations;
    assert!(rows.is_empty(), "the live branch's pending rows must not appear in a Snapshot: {rows:?}");
    assert!(!overlay.entity_exists(&snapshot, &entity_id).await.expect("snapshot existence"));
    let listed = overlay.query_entity_ids(&snapshot, &query()).await.expect("snapshot entity listing");
    assert!(listed.candidates.is_empty(), "the pending entity is the live branch's");
    assert!(
        overlay
            .entity_types(&snapshot, slice::from_ref(&entity_id))
            .await
            .expect("snapshot types")
            .is_empty()
    );

    // The same intent is visible on the live branch, so the Snapshot read
    // skipped it rather than finding nothing to overlay.
    assert!(overlay.entity_exists(&live_target(), &entity_id).await.expect("live existence"));
}

/// A Snapshot branch records its history in the write's own transaction, so
/// its mutation barrier has nothing queued to wait for and leaves the live
/// branch's pending intent for the entity alone.
#[tokio::test]
async fn a_snapshot_mutation_barrier_neither_waits_for_nor_delivers_live_intents() {
    let queue = Arc::new(RecordingQueue::default());
    let sink = Arc::new(RecordingSink::default());
    let (pending, entity_id) = fixture();
    queue.insert(pending).await;

    drain(Arc::clone(&queue), Arc::clone(&sink))
        .synchronize(&snapshot_target(), &entity_id)
        .await
        .expect("a Snapshot barrier has nothing to synchronize");

    assert_eq!(queue.pending_count().await, 1, "the live branch's intent stays queued");
    assert_eq!(sink.logical_effect_count().await, 0, "nothing was delivered on the Snapshot's behalf");
}
