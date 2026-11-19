//! The ports a recording entity service is wired to: a committer that keeps
//! the writes it saw, a reader that always serves one snapshot and counts the
//! states it hands out, and the ports the operations under test never reach.

use crate::{
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{
        attribute_history::AttributeDatasetHistory,
        blocking_executor::{BlockingExecutor, BlockingTask},
        reader::EntityReader,
        retrieved_entity::RetrievedEntity,
        spatial_catch_up_signal::SpatialCatchUpSignal,
    },
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    deleted_entity::DeletedEntity,
    entity_mutation::EntityMutation,
    entity_version::EntityVersion,
    error::CommitError,
    store_instant::StoreInstant,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_lock::{RwLock, RwLockWriteGuardArc};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::{
    channel::oneshot,
    future::{pending, try_join_all},
};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

/// What a [`RecordingCommitter`] saw: every write it was handed, the instant
/// each commit was accepted at, and how many commits arrived as respond-async
/// ones.
pub(crate) struct RecordedCommits {
    /// Every write, in the order the commits handed them over.
    pub(crate) writes: Arc<Mutex<Vec<AtomicWrite>>>,
    /// The instant each commit was handed, in commit order.
    pub(crate) accepted: Arc<Mutex<Vec<SystemInstant>>>,
    /// How many commits arrived through [`ContextCommitter::commit_enqueued`].
    pub(crate) enqueued: Arc<AtomicUsize>,
}

/// A committer that lands every write as an update and keeps the writes it saw.
pub(crate) struct RecordingCommitter {
    /// Every write it was handed.
    recorded: Arc<Mutex<Vec<AtomicWrite>>>,
    /// The instant each commit was handed.
    accepted: Arc<Mutex<Vec<SystemInstant>>>,
    /// How many commits arrived as respond-async ones.
    enqueued: Arc<AtomicUsize>,
}

impl RecordingCommitter {
    pub(crate) fn new() -> (RecordingCommitter, RecordedCommits) {
        let recorded: Arc<Mutex<Vec<AtomicWrite>>> = Arc::new(Mutex::new(Vec::new()));
        let accepted: Arc<Mutex<Vec<SystemInstant>>> = Arc::new(Mutex::new(Vec::new()));
        let enqueued = Arc::new(AtomicUsize::new(0));
        (
            RecordingCommitter {
                recorded: Arc::clone(&recorded),
                accepted: Arc::clone(&accepted),
                enqueued: Arc::clone(&enqueued),
            },
            RecordedCommits {
                writes: recorded,
                accepted,
                enqueued,
            },
        )
    }
}

impl ContextCommitter for RecordingCommitter {
    fn commit(&self, _target: &ContextTarget, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        self.accepted.lock().expect("recording mutex is not poisoned").push(accepted_at);
        // The outcome mirrors each write's mutation so a batch of inserts settles
        // as created, a deletion as deleted and everything else as updated,
        // which is what the batch coordinator reads back to build its
        // per-entity result.
        let now = Utc::now();
        let outcomes: Vec<WriteOutcome> = writes
            .iter()
            .map(|write| match write.entity {
                EntityMutation::Insert(_) => WriteOutcome::Created(StoreInstant::new(now)),
                EntityMutation::Update(_) | EntityMutation::None => WriteOutcome::Updated(StoreInstant::new(now)),
                EntityMutation::Delete => WriteOutcome::Deleted(
                    DeletedEntity {
                        id: Identifier::clone(&write.entity_id), // clone: the outcome owns the identifier the write shares
                        created_at: now,
                        modified_at: now,
                        expires_at: None,
                    },
                    StoreInstant::new(now),
                ),
            })
            .collect();
        self.recorded.lock().expect("recording mutex is not poisoned").extend(writes);
        Box::pin(async move { Ok(outcomes) })
    }

    fn commit_enqueued(&self, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        self.enqueued.fetch_add(1, Ordering::Relaxed);
        self.commit(&ContextTarget::Live, accepted_at, writes)
    }

    fn commit_replayed(&self, _target: &ContextTarget, _replay: CommitReplay, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        panic!("the durable-match recording tests never replay a commit")
    }
}

/// A reader that always hands back one Building snapshot for whatever is asked,
/// so both a single read-modify-write and a batch one have a state to compute
/// against.
pub(crate) struct SnapshotReader {
    /// The state every read serves.
    pub(crate) state: Arc<EntityState>,
    /// The modification instant every read serves.
    pub(crate) modified_at: DateTime<Utc>,
    /// The version every snapshot serves.
    pub(crate) version: EntityVersion,
    /// How many whole states were read, which a deletion reads only when a
    /// subscription may be owed its notification.
    pub(crate) payload_reads: Arc<AtomicUsize>,
}

impl SnapshotReader {
    fn sys_attrs(&self) -> SysAttrsSnapshot {
        SysAttrsSnapshot {
            created_at: self.modified_at,
            modified_at: self.modified_at,
            expires_at: None,
        }
    }
}

#[async_trait]
impl EntityReader for SnapshotReader {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        Ok(Some(Arc::clone(&self.state)))
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        Ok(ids
            .iter()
            .map(|id| {
                let retrieved = RetrievedEntity {
                    state: Arc::clone(&self.state),
                    sys_attrs: self.sys_attrs(),
                };
                (id.clone(), retrieved) // clone: the answer owns its keys
            })
            .collect())
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        Ok(Some(EntitySnapshot {
            state: Arc::clone(&self.state),
            version: self.version,
            sys_attrs: self.sys_attrs(),
        }))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        Ok(ids
            .iter()
            .map(|id| {
                (
                    id.clone(), // clone: the map owns each requested id
                    EntitySnapshot {
                        state: Arc::clone(&self.state),
                        version: self.version,
                        sys_attrs: self.sys_attrs(),
                    },
                )
            })
            .collect())
    }

    async fn get_with_sys_attrs(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        Ok(Some(RetrievedEntity {
            state: Arc::clone(&self.state),
            sys_attrs: self.sys_attrs(),
        }))
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        Ok(ids.iter().map(|id| (id.clone(), self.sys_attrs())).collect())
    }
}

/// A blocking executor the operations under test never reach.
pub(crate) struct UnusedBlockingExecutor;

#[async_trait]
impl BlockingExecutor for UnusedBlockingExecutor {
    async fn execute(&self, _tasks: Vec<Box<dyn BlockingTask>>, _concurrency: NonZeroUsize) -> Result<(), EntityError> {
        Ok(())
    }
}

/// A blocking executor that runs every task on the caller's thread, in order,
/// before the call returns.
pub(crate) struct InlineBlockingExecutor;

#[async_trait]
impl BlockingExecutor for InlineBlockingExecutor {
    async fn execute(&self, tasks: Vec<Box<dyn BlockingTask>>, _concurrency: NonZeroUsize) -> Result<(), EntityError> {
        tasks.into_iter().try_for_each(BlockingTask::run)
    }
}

/// A blocking executor that runs each task on a thread of its own once its
/// gate is open, as a blocking pool does. A caller that stops waiting leaves
/// the task to run to completion.
pub(crate) struct GatedBlockingExecutor {
    /// Open while no write guard is held on it.
    gate: Arc<RwLock<()>>,
    /// The write guard that keeps the gate closed.
    closed: Mutex<Option<RwLockWriteGuardArc<()>>>,
    /// How many tasks were handed over so far.
    queued: AtomicUsize,
}

impl GatedBlockingExecutor {
    /// An executor whose gate is open.
    pub(crate) fn open() -> GatedBlockingExecutor {
        GatedBlockingExecutor {
            gate: Arc::new(RwLock::new(())),
            closed: Mutex::new(None),
            queued: AtomicUsize::new(0),
        }
    }

    /// Hold every task handed over from now until [`GatedBlockingExecutor::release`].
    pub(crate) fn hold(&self) {
        let guard = self.gate.try_write_arc().expect("no task runs while the gate closes");
        *self.closed.lock().expect("the gate lock is not poisoned") = Some(guard);
    }

    /// Let every held task run.
    pub(crate) fn release(&self) {
        self.closed.lock().expect("the gate lock is not poisoned").take();
    }

    /// How many tasks were handed over so far.
    pub(crate) fn queued(&self) -> usize {
        self.queued.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl BlockingExecutor for GatedBlockingExecutor {
    async fn execute(&self, tasks: Vec<Box<dyn BlockingTask>>, _concurrency: NonZeroUsize) -> Result<(), EntityError> {
        let finished = tasks.into_iter().map(|task| {
            let (done, finished) = oneshot::channel();
            let gate = Arc::clone(&self.gate);
            self.queued.fetch_add(1, Ordering::SeqCst);
            // The gate is released before the outcome is sent, so a caller that
            // closes the gate once `execute` returns never finds it still read.
            thread::spawn(move || {
                let outcome = {
                    let _open = gate.read_blocking();
                    task.run()
                };
                let _ignored = done.send(outcome);
            });
            finished
        });
        let outcomes = try_join_all(finished).await.expect("every task thread reports back");
        outcomes.into_iter().collect()
    }
}

/// A spatial catch-up signal that counts its wakes.
#[derive(Default)]
pub(crate) struct RecordingSignal {
    /// How many wakes were asked for.
    wakes: AtomicUsize,
}

impl RecordingSignal {
    /// How many wakes were asked for so far.
    pub(crate) fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }
}

impl SpatialCatchUpSignal for RecordingSignal {
    fn wake(&self) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }
}

/// What a [`ScriptedCommitter`] does with every commit it is handed.
pub(crate) enum Script {
    /// Every write lands with this outcome.
    Answer(fn() -> WriteOutcome),
    /// The commit never completes, as when the request is dropped while the
    /// store is committing.
    Hang,
    /// The commit fails as an unreachable store does.
    Fail,
}

/// A committer answering every commit by its script.
pub(crate) struct ScriptedCommitter(pub(crate) Script);

impl ContextCommitter for ScriptedCommitter {
    fn commit(&self, _target: &ContextTarget, _accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        match self.0 {
            Script::Answer(outcome) => Box::pin(async move { Ok(writes.iter().map(|_| outcome()).collect()) }),
            Script::Hang => Box::pin(pending()),
            Script::Fail => Box::pin(async { Err(CommitError::Persistence(PersistenceFailure::Unavailable)) }),
        }
    }

    fn commit_enqueued(&self, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        self.commit(&ContextTarget::Live, accepted_at, writes)
    }

    fn commit_replayed(&self, target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        self.commit(target, SystemInstant::new(replay.record.recorded_at()), writes)
    }
}

/// An attribute-history port only `deleteAttr` with `DatasetSelection::All`
/// consults, which these tests avoid.
pub(crate) struct UnusedAttributeHistory;

#[async_trait]
impl AttributeDatasetHistory for UnusedAttributeHistory {
    async fn dataset_identifiers(
        &self,
        _target: &ContextTarget,
        _entity: &Identifier,
        _attribute: &AttributeName,
    ) -> Result<Vec<Option<Identifier>>, EntityError> {
        Ok(Vec::new())
    }
}
