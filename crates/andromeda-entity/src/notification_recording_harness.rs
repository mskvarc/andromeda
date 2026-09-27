//! A recording notification harness: an [`EntityService`] wired to a committer
//! that keeps the writes it saw and a reader that always serves one Building
//! snapshot, with one matching Building subscription in its cache.
//!
//! It proves — without a live broker — that an operation records the durable
//! subscription matches its own live notification would produce (clause 5.8):
//! a test commits an operation, reads the `subscription_matches` off the
//! [`AtomicWrite`] the committer captured, and asserts the trigger,
//! `changedAttributes` and snapshot a matching subscription is owed. The single
//! reader serves both the single-entity and the batch read ports, so the
//! whole-entity and attribute-level paths share one fixture.

use crate::{
    EntityService,
    NotificationWriteConfig,
    batch::execution::BatchExecutionConfig,
    building_subscription_fixture::{BUILDING_TYPE_IRI, building_subscription_cache, subscription_key},
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{
        attribute_history::AttributeDatasetHistory,
        blocking_executor::{BlockingExecutor, BlockingTask},
        reader::EntityReader,
        retry_delay::RetryDelay,
        service_ports::EntityServicePorts,
    },
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    write_outcome::WriteOutcome,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::{NotificationTrigger, SysAttrsSnapshot};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
    sync::{Arc, Mutex},
    time::Duration,
};

/// The entity every operation under test reads, mutates and writes back.
pub(crate) const ENTITY_ID: &str = "urn:ngsi-ld:Building:durable-test";
/// The expanded IRI the `category` term the entity already carries resolves to.
pub(crate) const CATEGORY_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/category";
/// The expanded IRI the appended `name` term resolves to.
pub(crate) const NAME_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/name";
/// The expanded IRI of the sub-attribute the stored `category` carries, used to
/// prove a whole-instance overwrite drops an unmentioned sub-attribute.
pub(crate) const ACCURACY_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/accuracy";

/// A committer that lands every write as an update and keeps the writes it saw.
struct RecordingCommitter {
    recorded: Arc<Mutex<Vec<AtomicWrite>>>,
}

impl RecordingCommitter {
    fn new() -> (RecordingCommitter, Arc<Mutex<Vec<AtomicWrite>>>) {
        let recorded: Arc<Mutex<Vec<AtomicWrite>>> = Arc::new(Mutex::new(Vec::new()));
        (
            RecordingCommitter {
                recorded: Arc::clone(&recorded), // clone: the committer and the test both hold the recording.
            },
            recorded,
        )
    }
}

impl ContextCommitter for RecordingCommitter {
    fn commit(&self, _target: &ContextTarget, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        // The outcome mirrors each write's mutation so a batch of inserts settles
        // as created and everything else as updated, which is what the batch
        // coordinator reads back to build its per-entity result.
        let outcomes: Vec<WriteOutcome> = writes
            .iter()
            .map(|write| {
                if write.entity.is_insert() {
                    WriteOutcome::Created
                } else {
                    WriteOutcome::Updated
                }
            })
            .collect();
        self.recorded.lock().expect("recording mutex is not poisoned").extend(writes);
        Box::pin(async move { Ok(outcomes) })
    }

    fn commit_replayed(&self, _target: &ContextTarget, _replay: CommitReplay, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        panic!("the durable-match recording tests never replay a commit")
    }
}

/// A reader that always hands back one Building snapshot for whatever is asked,
/// so both a single read-modify-write and a batch one have a payload to compute
/// against.
struct SnapshotReader {
    payload: Value,
    modified_at: DateTime<Utc>,
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
    async fn get(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<Value>, EntityError> {
        Ok(Some(self.payload.clone())) // clone: the caller owns its copy while the reader keeps serving the snapshot.
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Uri]) -> Result<HashMap<Uri, Value>, EntityError> {
        // clone: each requested id gets its own owned copy of the fixture.
        Ok(ids.iter().map(|id| (id.clone(), self.payload.clone())).collect())
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<EntitySnapshot>, EntityError> {
        Ok(Some(EntitySnapshot {
            payload: self.payload.clone(), // clone: each read yields an owned snapshot the writer mutates.
            modified_at: self.modified_at,
        }))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, ids: &[Uri]) -> Result<HashMap<Uri, EntitySnapshot>, EntityError> {
        Ok(ids
            .iter()
            .map(|id| {
                (
                    id.clone(), // clone: the map owns each requested id
                    EntitySnapshot {
                        payload: self.payload.clone(), // clone: each entry owns its snapshot
                        modified_at: self.modified_at,
                    },
                )
            })
            .collect())
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Ok(Some(self.sys_attrs()))
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, ids: &[Uri]) -> Result<HashMap<Uri, SysAttrsSnapshot>, EntityError> {
        Ok(ids.iter().map(|id| (id.clone(), self.sys_attrs())).collect())
    }
}

/// A retry delay that never waits, because the recording committer never loses.
struct ImmediateRetry;

#[async_trait]
impl RetryDelay for ImmediateRetry {
    async fn wait(&self, _delay: Duration) {}
}

/// A blocking executor the operations under test never reach.
struct UnusedBlockingExecutor;

#[async_trait]
impl BlockingExecutor for UnusedBlockingExecutor {
    async fn execute(&self, _tasks: Vec<Box<dyn BlockingTask>>, _concurrency: NonZeroUsize) -> Result<(), EntityError> {
        Ok(())
    }
}

/// An attribute-history port only `deleteAttr` with `DatasetSelection::All`
/// consults, which these tests avoid.
struct UnusedAttributeHistory;

#[async_trait]
impl AttributeDatasetHistory for UnusedAttributeHistory {
    async fn dataset_identifiers(&self, _target: &ContextTarget, _entity: &Uri, _attribute: &AttributeName) -> Result<Vec<Option<Uri>>, EntityError> {
        Ok(Vec::new())
    }
}

/// The Building payload the reader returns, expanded and carrying one `category`
/// Property so the update, partial-update, delete and replace operations have an
/// attribute to act on.
pub(crate) fn building_snapshot() -> Value {
    json!({
        "@id": ENTITY_ID,
        "@type": [BUILDING_TYPE_IRI],
        CATEGORY_IRI: [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "office"}],
            ACCURACY_IRI: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "high"}]
            }]
        }]
    })
}

/// An entity service of a durable-delivery broker wired to the recording
/// committer, with the one Building subscription in its cache, returning the
/// shared recording buffer.
pub(crate) async fn recording_service() -> (EntityService, Arc<Mutex<Vec<AtomicWrite>>>) {
    recording_service_with(Some(NotificationWriteConfig {
        subscription_cache: building_subscription_cache().await,
    }))
}

/// An entity service of a live-delivery broker wired to the recording
/// committer: it records no durable match, because its notification worker
/// evaluates every change in-process.
pub(crate) fn live_recording_service() -> (EntityService, Arc<Mutex<Vec<AtomicWrite>>>) {
    recording_service_with(None)
}

/// An entity service over the recording committer that records durable matches
/// through `notifications` when given.
fn recording_service_with(notifications: Option<NotificationWriteConfig>) -> (EntityService, Arc<Mutex<Vec<AtomicWrite>>>) {
    let (committer, recorded) = RecordingCommitter::new();
    let service = EntityService::new(
        EntityServicePorts {
            reader: Arc::new(SnapshotReader {
                payload: building_snapshot(),
                modified_at: Utc::now(),
            }) as Arc<dyn EntityReader>,
            blocking_executor: Arc::new(UnusedBlockingExecutor) as Arc<dyn BlockingExecutor>,
            retry_delay: Arc::new(ImmediateRetry) as Arc<dyn RetryDelay>,
            attribute_history: Arc::new(UnusedAttributeHistory) as Arc<dyn AttributeDatasetHistory>,
            writes: Arc::new(committer) as Arc<dyn ContextCommitter>,
        },
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds"),
        None,
        BatchExecutionConfig {
            concurrency: 1,
            chunk_concurrency: 1,
            chunk_write_concurrency: 1,
            chunk_size: 1,
            large_threshold: 1,
            // Above any batch these tests submit, so preparation stays inline on
            // the calling task rather than offloading to the stub blocking pool.
            prepare_blocking_threshold: 1024,
            async_side_effect_threshold: None,
        },
        notifications,
    );
    (service, recorded)
}

/// The ordinary branch target used by the recording fixtures.
pub(crate) fn live_target() -> ContextTarget {
    ContextTarget::Live
}

/// The fixture entity identifier as a [`Uri`].
pub(crate) fn entity_uri() -> Uri {
    ENTITY_ID.parse().expect("the fixture entity identifier is a valid URI")
}

/// Parse an expanded attribute IRI into an [`AttributeName`].
pub(crate) fn attribute_name(iri: &str) -> AttributeName {
    iri.parse().expect("the fixture attribute IRI is a valid name")
}

/// The empty context: the fixtures are already expanded, so no terms resolve.
pub(crate) fn core_context() -> ContextSpec {
    ContextSpec::none()
}

/// Take the single write the operation committed, failing loudly otherwise.
pub(crate) fn single_recorded_write(recorded: &Arc<Mutex<Vec<AtomicWrite>>>) -> AtomicWrite {
    let mut writes = recorded.lock().expect("recording mutex is not poisoned");
    assert_eq!(writes.len(), 1, "the operation must commit exactly one entity write");
    writes.remove(0)
}

/// Assert the write is owed exactly one match with the given trigger and changed
/// attribute, carrying the post-mutation snapshot.
pub(crate) fn assert_single_match(write: &AtomicWrite, trigger: NotificationTrigger, changed: &str) {
    assert_eq!(
        write.subscription_matches.len(),
        1,
        "the matching subscription is owed exactly one notification"
    );
    let matched = &write.subscription_matches[0];
    assert_eq!(matched.subscription_key, subscription_key());
    assert_eq!(matched.entity_id.as_str(), ENTITY_ID);
    assert_eq!(matched.trigger, trigger);
    assert_eq!(matched.changed_attrs, vec![attribute_name(changed)]);
    assert!(matched.entity_snapshot.is_some(), "the durable match carries the post-mutation entity snapshot");
}

/// Assert the write is owed exactly the given `(trigger, changed attribute)`
/// matches, as an order-independent set: a whole-entity mutation records one
/// match per changed instance and their order is not spec-significant. Each
/// match must name exactly one changed attribute and carry the post-mutation
/// snapshot against the fixture subscription.
pub(crate) fn assert_matches(write: &AtomicWrite, expected: &[(NotificationTrigger, &str)]) {
    let actual: HashSet<(NotificationTrigger, AttributeName)> = write
        .subscription_matches
        .iter()
        .map(|matched| {
            assert_eq!(matched.subscription_key, subscription_key());
            assert_eq!(matched.entity_id.as_str(), ENTITY_ID);
            assert!(matched.entity_snapshot.is_some(), "the durable match carries the post-mutation entity snapshot");
            assert_eq!(matched.changed_attrs.len(), 1, "each whole-entity match names exactly one changed attribute");
            (matched.trigger, matched.changed_attrs[0].clone()) // clone: the set owns each changed attribute name
        })
        .collect();
    let expected: HashSet<(NotificationTrigger, AttributeName)> = expected.iter().map(|(trigger, changed)| (*trigger, attribute_name(changed))).collect();
    assert_eq!(actual, expected);
}
