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
    },
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    deleted_entity::DeletedEntity,
    entity_mutation::EntityMutation,
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use andromeda_retry::retry_delay::RetryDelay;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
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
                recorded: Arc::clone(&recorded), // clone: the committer and the test both hold the recording.
                accepted: Arc::clone(&accepted), // clone: the committer and the test both hold the instants.
                enqueued: Arc::clone(&enqueued), // clone: the committer and the test both hold the count.
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
                EntityMutation::Insert(_) => WriteOutcome::Created,
                EntityMutation::Update(_) | EntityMutation::None => WriteOutcome::Updated,
                EntityMutation::Delete => WriteOutcome::Deleted(DeletedEntity {
                    id: Identifier::clone(&write.entity_id), // clone: the outcome owns the identifier the write shares
                    created_at: now,
                    modified_at: now,
                    expires_at: None,
                }),
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
        Ok(Some(Arc::clone(&self.state))) // clone: Arc ref-count bump; the reader keeps serving the snapshot.
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        Ok(ids
            .iter()
            .map(|id| {
                let retrieved = RetrievedEntity {
                    state: Arc::clone(&self.state), // clone: Arc ref-count bump; the reader keeps serving the snapshot.
                    sys_attrs: self.sys_attrs(),
                };
                (id.clone(), retrieved) // clone: the answer owns its keys
            })
            .collect())
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        self.payload_reads.fetch_add(1, Ordering::Relaxed);
        Ok(Some(EntitySnapshot {
            state: Arc::clone(&self.state), // clone: Arc ref-count bump; a writer copies what it changes.
            modified_at: self.modified_at,
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
                        state: Arc::clone(&self.state), // clone: Arc ref-count bump; a writer copies what it changes
                        modified_at: self.modified_at,
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
            state: Arc::clone(&self.state), // clone: Arc ref-count bump; the reader keeps serving the snapshot.
            sys_attrs: self.sys_attrs(),
        }))
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Ok(Some(self.sys_attrs()))
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        Ok(ids.iter().map(|id| (id.clone(), self.sys_attrs())).collect())
    }
}

/// A retry delay that never waits, because the recording committer never loses.
pub(crate) struct ImmediateRetry;

#[async_trait]
impl RetryDelay for ImmediateRetry {
    async fn wait(&self, _delay: Duration) {}
}

/// A blocking executor the operations under test never reach.
pub(crate) struct UnusedBlockingExecutor;

#[async_trait]
impl BlockingExecutor for UnusedBlockingExecutor {
    async fn execute(&self, _tasks: Vec<Box<dyn BlockingTask>>, _concurrency: NonZeroUsize) -> Result<(), EntityError> {
        Ok(())
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
