//! Every attribute-level write records the durable subscription matches its own
//! live notification would produce (clause 5.8), proven without a live broker.
//!
//! Each of the five clause 5.6.2 to 5.6.5 and 5.6.19 operations computes its
//! notification inside the same closure that computes the write. These tests
//! commit each operation against a recording committer and read the
//! `subscription_matches` off the [`AtomicWrite`] it produced, so a matching
//! subscription is owed exactly the trigger, `changedAttributes` and snapshot
//! the operation's live emit would carry.

use crate::{
    EntityService,
    NotificationWriteConfig,
    batch::execution::BatchExecutionConfig,
    batch_activity::BatchActivityTracker,
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{
        attribute_history::AttributeDatasetHistory,
        blocking_executor::{BlockingExecutor, BlockingTask},
        reader::EntityReader,
        retry_delay::RetryDelay,
    },
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
    subscription_key::SubscriptionKey,
    write_outcome::WriteOutcome,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::mutation::selection::{DatasetSelection, OverwritePolicy},
    notification::{NotificationTrigger, SysAttrsSnapshot},
    subscription::{Subscription, SubscriptionStatus},
};
use andromeda_subscription::{
    cache::SubscriptionCache,
    error::SubscriptionError,
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
    time::Duration,
};

/// The entity every operation under test reads, mutates and writes back.
const ENTITY_ID: &str = "urn:ngsi-ld:Building:durable-test";
/// The subscription the recording committer must be owed a match against.
const SUBSCRIPTION_ID: &str = "urn:ngsi-ld:Subscription:durable-test";
/// The surrogate key the fixture subscription is stored under.
const SUBSCRIPTION_KEY: i64 = 1;
/// The expanded Building type both the entity and the subscription selector carry.
const BUILDING_TYPE_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/Building";
/// The expanded IRI the `category` term the entity already carries resolves to.
const CATEGORY_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/category";
/// The expanded IRI the appended `name` term resolves to.
const NAME_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/name";

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
    fn commit(&self, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let count = writes.len();
        self.recorded.lock().expect("recording mutex is not poisoned").extend(writes);
        // WriteOutcome is not Clone, so the per-write outcomes are built rather
        // than repeated; `Updated` settles an `UnchangedSince` write on the
        // first attempt, so the operation lands without a re-read.
        Box::pin(async move { Ok((0..count).map(|_| WriteOutcome::Updated).collect()) })
    }

    fn commit_replayed(&self, _replay: CommitReplay, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        unreachable!("the durable-match recording tests never replay a commit")
    }
}

/// A reader that always hands back one Building snapshot, so a read-modify-write
/// has a payload to compute against.
struct SnapshotReader {
    payload: Value,
    modified_at: DateTime<Utc>,
}

#[async_trait]
impl EntityReader for SnapshotReader {
    async fn get(&self, _id: &Uri) -> Result<Option<Value>, EntityError> {
        Ok(Some(self.payload.clone())) // clone: the caller owns its copy while the reader keeps serving the snapshot.
    }

    async fn get_batch(&self, _ids: &[Uri]) -> Result<HashMap<Uri, Value>, EntityError> {
        Ok(HashMap::new())
    }

    async fn get_snapshot(&self, _id: &Uri) -> Result<Option<EntitySnapshot>, EntityError> {
        Ok(Some(EntitySnapshot {
            payload: self.payload.clone(), // clone: each read yields an owned snapshot the writer mutates.
            modified_at: self.modified_at,
        }))
    }

    async fn get_snapshot_batch(&self, _ids: &[Uri]) -> Result<HashMap<Uri, EntitySnapshot>, EntityError> {
        Ok(HashMap::new())
    }

    async fn get_sys_attrs(&self, _id: &Uri) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Ok(None)
    }

    async fn get_sys_attrs_batch(&self, _ids: &[Uri]) -> Result<HashMap<Uri, SysAttrsSnapshot>, EntityError> {
        Ok(HashMap::new())
    }
}

/// A retry delay that never waits, because the recording committer never loses.
struct ImmediateRetry;

#[async_trait]
impl RetryDelay for ImmediateRetry {
    async fn wait(&self, _delay: Duration) {}
}

/// A blocking executor the attribute-level operations never reach.
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
    async fn dataset_identifiers(&self, _entity: &Uri, _attribute: &AttributeName) -> Result<Vec<Option<Uri>>, EntityError> {
        Ok(Vec::new())
    }
}

/// A subscription store whose only exercised method is `list_active`, which the
/// cache reloads once to learn the one matching Building subscription.
struct SingleSubscriptionStore;

fn unused_store_call(method: &str) -> SubscriptionError {
    SubscriptionError::internal(format!(
        "SingleSubscriptionStore::{method} is not exercised by the durable-match recording tests"
    ))
}

/// Build the one stored Building subscription the cache matches against.
///
/// It is stored in expanded form (its selector carries the expanded Building
/// IRI), omits `notificationTrigger` so it matches every trigger, and is created
/// in the past so the clause 5.2.12 `createdAt` guard admits the tests' events.
fn stored_building_subscription() -> Result<StoredSubscription, SubscriptionError> {
    let subscription = serde_json::from_value::<Subscription>(json!({
        "id": SUBSCRIPTION_ID,
        "type": "Subscription",
        "entities": [{"type": BUILDING_TYPE_IRI}],
        "notification": {"endpoint": {"uri": "http://localhost/notify"}},
        "status": "active",
        "createdAt": "2020-01-01T00:00:00Z",
        "modifiedAt": "2020-01-01T00:00:00Z"
    }))
    .map_err(|error| SubscriptionError::internal(format!("invalid fixture subscription: {error}")))?;
    let key = SubscriptionKey::try_from(SUBSCRIPTION_KEY).map_err(|error| SubscriptionError::internal(format!("invalid fixture subscription key: {error}")))?;
    Ok(StoredSubscription { key, subscription })
}

/// The fixture subscription's surrogate key, as the store would have assigned it.
fn subscription_key() -> SubscriptionKey {
    SubscriptionKey::try_from(SUBSCRIPTION_KEY).expect("the fixture subscription key is positive")
}

#[async_trait]
impl SubscriptionStore for SingleSubscriptionStore {
    async fn get(&self, _id: &Uri) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Err(unused_store_call("get"))
    }

    async fn list(&self, _limit: Option<i64>, _offset: Option<i64>) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused_store_call("list"))
    }

    async fn delete(&self, _id: &Uri) -> Result<u64, SubscriptionError> {
        Err(unused_store_call("delete"))
    }

    async fn exists(&self, _id: &Uri) -> Result<bool, SubscriptionError> {
        Err(unused_store_call("exists"))
    }

    async fn list_active(&self) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Ok(vec![stored_building_subscription()?])
    }

    async fn claim_notification_slot(&self, _id: &Uri, _earliest_allowed_last_notification: Option<DateTime<Utc>>) -> Result<bool, SubscriptionError> {
        Err(unused_store_call("claim_notification_slot"))
    }

    async fn mark_success(&self, _id: &Uri) -> Result<(), SubscriptionError> {
        Err(unused_store_call("mark_success"))
    }

    async fn mark_failure(&self, _id: &Uri) -> Result<(), SubscriptionError> {
        Err(unused_store_call("mark_failure"))
    }

    async fn expire_due(&self, _cutoff: DateTime<Utc>) -> Result<Vec<Uri>, SubscriptionError> {
        Err(unused_store_call("expire_due"))
    }

    async fn find_due_interval_subscriptions(&self) -> Result<Vec<Subscription>, SubscriptionError> {
        Err(unused_store_call("find_due_interval_subscriptions"))
    }

    async fn create_subscription(&self, _id: &Uri, _fields: &SubscriptionFields<'_>, _status: SubscriptionStatus) -> Result<(), SubscriptionError> {
        Err(unused_store_call("create_subscription"))
    }

    async fn update_subscription(&self, _id: &Uri, _fields: &SubscriptionFields<'_>, _status: SubscriptionStatus) -> Result<u64, SubscriptionError> {
        Err(unused_store_call("update_subscription"))
    }
}

/// The Building payload the reader returns, expanded and carrying one `category`
/// Property so the update, partial-update, delete and replace operations have an
/// attribute to act on.
fn building_snapshot() -> Value {
    json!({
        "@id": ENTITY_ID,
        "@type": [BUILDING_TYPE_IRI],
        CATEGORY_IRI: [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "office"}]
        }]
    })
}

/// An entity service wired to the recording committer, with the one Building
/// subscription in its cache, returning the shared recording buffer.
async fn recording_service() -> (EntityService, Arc<Mutex<Vec<AtomicWrite>>>) {
    let (committer, recorded) = RecordingCommitter::new();
    let cache = Arc::new(SubscriptionCache::new(TenantRef::UnnamedDefault));
    cache.reload(&SingleSubscriptionStore).await.expect("the fixture subscription reloads");

    let service = EntityService::new(
        Arc::new(SnapshotReader {
            payload: building_snapshot(),
            modified_at: Utc::now(),
        }) as Arc<dyn EntityReader>,
        Arc::new(UnusedBlockingExecutor) as Arc<dyn BlockingExecutor>,
        Arc::new(ImmediateRetry) as Arc<dyn RetryDelay>,
        Arc::new(UnusedAttributeHistory) as Arc<dyn AttributeDatasetHistory>,
        Arc::new(committer) as Arc<dyn ContextCommitter>,
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds"),
        None,
        BatchExecutionConfig {
            concurrency: 1,
            chunk_concurrency: 1,
            chunk_write_concurrency: 1,
            chunk_size: 1,
            large_threshold: 1,
            prepare_blocking_threshold: 1,
            async_side_effect_threshold: None,
        },
        BatchActivityTracker::new(),
        Some(NotificationWriteConfig {
            subscription_cache: Arc::clone(&cache), // clone: the service and the test each hold the cache.
        }),
    );
    (service, recorded)
}

fn entity_uri() -> Uri {
    ENTITY_ID.parse().expect("the fixture entity identifier is a valid URI")
}

fn attribute_name(iri: &str) -> AttributeName {
    iri.parse().expect("the fixture attribute IRI is a valid name")
}

fn core_context() -> ContextSpec {
    ContextSpec::none()
}

/// Take the single write the operation committed, failing loudly otherwise.
fn single_recorded_write(recorded: &Arc<Mutex<Vec<AtomicWrite>>>) -> AtomicWrite {
    let mut writes = recorded.lock().expect("recording mutex is not poisoned");
    assert_eq!(writes.len(), 1, "the operation must commit exactly one entity write");
    writes.remove(0)
}

/// Assert the write is owed exactly one match with the given trigger and changed
/// attribute, carrying the post-mutation snapshot.
fn assert_single_match(write: &AtomicWrite, trigger: NotificationTrigger, changed: &str) {
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

#[tokio::test]
async fn append_attrs_records_an_attribute_created_match() {
    let (service, recorded) = recording_service().await;
    service
        .append_attrs(
            &entity_uri(),
            json!({"name": {"type": "Property", "value": "lobby"}}),
            OverwritePolicy::Overwrite,
            &core_context(),
        )
        .await
        .expect("the append lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeCreated, NAME_IRI);
}

#[tokio::test]
async fn update_attrs_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .update_attrs(&entity_uri(), json!({"category": {"type": "Property", "value": "retail"}}), &core_context())
        .await
        .expect("the update lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn partial_update_attr_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .partial_update_attr(
            &entity_uri(),
            &attribute_name("category"),
            None,
            json!({"type": "Property", "value": "retail"}),
            &core_context(),
        )
        .await
        .expect("the partial update lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn delete_attr_records_an_attribute_deleted_match() {
    let (service, recorded) = recording_service().await;
    service
        .delete_attr(&entity_uri(), &attribute_name("category"), DatasetSelection::Default, &core_context())
        .await
        .expect("the delete lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeDeleted, CATEGORY_IRI);
}

#[tokio::test]
async fn replace_attr_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .replace_attr(
            &entity_uri(),
            &attribute_name("category"),
            None,
            json!({"type": "Property", "value": "retail"}),
            &core_context(),
        )
        .await
        .expect("the replace lands");

    let write = single_recorded_write(&recorded);
    assert_single_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}
