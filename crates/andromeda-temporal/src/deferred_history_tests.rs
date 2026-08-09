use crate::{
    append_intent::{
        SequencedTemporalAppendIntent,
        TemporalAppendEntry,
        TemporalAppendIntent,
        TemporalIntentId,
        TemporalIntentSequence,
        TemporalObservationIdentity,
    },
    error::TemporalError,
    intent_drain::TemporalIntentDrain,
    outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    pending_overlay::PendingTemporalHistoryOverlay,
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            TemporalHistoryObservation,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        mutation_barrier::TemporalMutationBarrier,
        pending_intent_reader::PendingTemporalIntentReader,
        temporal_history_sink::{TemporalHistorySink, TemporalSinkAppendOutcome},
        temporal_intent_queue::{ClaimedTemporalAppendIntent, TemporalIntentBatchSize, TemporalIntentLeaseOwner, TemporalIntentQueue, TemporalIntentRetry},
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    temporal::{compiled::CompiledTemporalQuery, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use std::{collections::HashMap, num::NonZeroU32, sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Default)]
struct RecordingQueueState {
    pending: Vec<ClaimedTemporalAppendIntent>,
    acknowledged: Vec<TemporalIntentId>,
    retries: Vec<TemporalIntentRetry>,
}

#[derive(Default)]
struct RecordingQueue {
    state: Mutex<RecordingQueueState>,
}

impl RecordingQueue {
    async fn insert(&self, pending: SequencedTemporalAppendIntent) {
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
}

#[async_trait]
impl PendingTemporalIntentReader for RecordingQueue {
    async fn pending_for_entity(&self, entity_id: &Uri) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError> {
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

    async fn pending_intents(&self) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError> {
        Ok(self.state.lock().await.pending.iter().map(|claim| claim.pending.clone()).collect())
    }
}

#[async_trait]
impl TemporalIntentQueue for RecordingQueue {
    async fn claim(
        &self,
        _owner: TemporalIntentLeaseOwner,
        limit: TemporalIntentBatchSize,
        _lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError> {
        let limit = usize::try_from(limit.get()).unwrap_or(usize::MAX);
        Ok(self.state.lock().await.pending.iter().take(limit).cloned().collect())
    }

    async fn claim_for_entity(
        &self,
        _owner: TemporalIntentLeaseOwner,
        entity_id: &Uri,
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

    async fn acknowledge(&self, _owner: TemporalIntentLeaseOwner, intent: &SequencedTemporalAppendIntent) -> Result<(), TemporalError> {
        let mut state = self.state.lock().await;
        state.pending.retain(|claim| claim.pending.intent.id != intent.intent.id);
        state.acknowledged.push(intent.intent.id);
        Ok(())
    }

    async fn retry(&self, _owner: TemporalIntentLeaseOwner, intent: &SequencedTemporalAppendIntent, retry: TemporalIntentRetry) -> Result<(), TemporalError> {
        let mut state = self.state.lock().await;
        if let Some(claim) = state.pending.iter_mut().find(|claim| claim.pending.intent.id == intent.intent.id) {
            claim.attempts = claim.attempts.saturating_add(1);
        }
        state.retries.push(retry);
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSinkState {
    observations: HashMap<TemporalIntentId, (Uri, Vec<TemporalHistoryObservation>)>,
    reject: bool,
}

#[derive(Default)]
struct RecordingSink {
    state: Mutex<RecordingSinkState>,
}

impl RecordingSink {
    async fn reject(&self) {
        self.state.lock().await.reject = true;
    }

    async fn logical_effect_count(&self) -> usize {
        self.state.lock().await.observations.len()
    }
}

#[async_trait]
impl TemporalHistorySink for RecordingSink {
    async fn append(&self, intent: &TemporalAppendIntent) -> Result<TemporalSinkAppendOutcome, TemporalError> {
        let mut state = self.state.lock().await;
        if state.reject {
            return Err(TemporalError::persistence("recording sink rejected append"));
        }
        if state.observations.contains_key(&intent.id) {
            return Ok(TemporalSinkAppendOutcome::AlreadyPresent);
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
                    instance_id,
                    recorded_at,
                } => Some(TemporalHistoryObservation {
                    row: TemporalHistoryRow {
                        attribute_name: attribute_name.clone(), // clone: the recording sink owns the tombstone identity.
                        attribute_type: AttributeType::Property,
                        dataset_id: dataset_id.clone(), // clone: the recording sink owns the dataset identity.
                        value: TemporalValue::Tombstone,
                        observed_at: None,
                        time: *recorded_at,
                        created_at: *recorded_at,
                        modified_at: *recorded_at,
                        instance_id: instance_id.clone(), // clone: the recording sink owns the instance identity.
                        metadata: None,
                        partition_total: None,
                    },
                    identity: Some(identity),
                }),
                TemporalAppendEntry::Scope { .. } => None,
            })
            .collect();
        state.observations.insert(intent.id, (intent.entity_id.clone(), observations)); // clone: the recording sink owns the entity identity.
        Ok(TemporalSinkAppendOutcome::Appended)
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

    async fn entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalError> {
        Ok(self.state.lock().await.observations.values().any(|(stored_id, _)| stored_id == entity_id))
    }

    async fn query_entity_ids(&self, _compiled: &CompiledTemporalQuery) -> Result<TemporalEntityIdOutcome, TemporalError> {
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
                .map(|entity_id| TemporalEntityCandidate {
                    entity_id,
                    selection_value: None,
                })
                .collect(),
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Deferred,
        })
    }

    async fn query_entity_history(&self, entity_id: &Uri, _compiled: &CompiledTemporalQuery) -> Result<TemporalHistoryOutcome, TemporalError> {
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

    async fn query_entity_aggregation(&self, _entity_id: &Uri, _compiled: &CompiledTemporalQuery) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        Err(TemporalError::operation_not_supported("recording sink has no aggregation"))
    }
}

fn fixture() -> (SequencedTemporalAppendIntent, Uri) {
    let entity_id = "urn:ngsi-ld:Vehicle:deferred".parse::<Uri>().expect("valid entity ID");
    let intent_id = TemporalIntentId::mint();
    let row = TemporalHistoryRow {
        attribute_name: "https://example.com/speed".parse().expect("valid attribute name"),
        attribute_type: AttributeType::Property,
        dataset_id: None,
        value: TemporalValue::Number(42.0),
        observed_at: Some(instant()),
        time: instant(),
        created_at: instant(),
        modified_at: instant(),
        instance_id: format!("urn:ngsi-ld:attribute:instance:{intent_id}:0").parse().expect("valid instance ID"),
        metadata: None,
        partition_total: None,
    };
    let pending = SequencedTemporalAppendIntent {
        sequence: TemporalIntentSequence::new(1).expect("positive sequence"),
        intent: TemporalAppendIntent {
            id: intent_id,
            entity_id: entity_id.clone(), // clone: fixture returns the identifier separately from the intent.
            entries: vec![TemporalAppendEntry::Observation {
                row,
                client_supplied_instance_id: false,
                recorded_at: instant(),
            }],
            selection_value: Some(serde_json::json!([{"@id": entity_id}])),
        },
    };
    (pending, entity_id)
}

fn instant() -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant")
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

fn drain(queue: Arc<RecordingQueue>, sink: Arc<RecordingSink>) -> TemporalIntentDrain {
    TemporalIntentDrain::new(
        Arc::clone(&queue) as Arc<dyn TemporalIntentQueue>,
        queue as Arc<dyn PendingTemporalIntentReader>,
        sink,
        TemporalIntentLeaseOwner::mint(),
        TemporalIntentBatchSize::new(NonZeroU32::new(8).expect("non-zero batch")),
        Duration::from_secs(30),
        Duration::from_secs(2),
    )
}

async fn overlay_rows(queue: Arc<RecordingQueue>, sink: Arc<RecordingSink>, entity_id: &Uri) -> Vec<TemporalHistoryObservation> {
    PendingTemporalHistoryOverlay::new(sink, queue)
        .query_entity_history(entity_id, &query())
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
    assert!(overlay.entity_exists(&entity_id).await.expect("logical entity existence"));
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
        .synchronize(&entity_id)
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
    sink.append(&pending.intent).await.expect("sink accepts intent");
    let rows = overlay_rows(queue, sink, &entity_id).await;
    assert_eq!(rows.len(), 1);
    pretty_assertions::assert_eq!(
        rows[0].identity,
        Some(TemporalObservationIdentity {
            intent_id: pending.intent.id,
            ordinal: 0
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
    sink.append(&pending.intent).await.expect("first sink effect succeeds");
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
    assert_eq!(queue.retry_count().await, 1);
    assert_eq!(queue.pending_count().await, 1);
    assert_eq!(queue.acknowledged_count().await, 0);
    assert_eq!(overlay_rows(queue, sink, &entity_id).await.len(), 1);
}
