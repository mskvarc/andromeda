//! A recording notification harness: an [`EntityService`] wired to a committer
//! that keeps the writes it saw and a reader that always serves one Building
//! snapshot, with one matching Building subscription in its cache.
//!
//! It proves, without a live broker, that an operation records the durable
//! subscription matches its own live notification would produce (clause 5.8):
//! a test commits an operation, reads the `subscription_matches` off the
//! [`AtomicWrite`] the committer captured, and asserts the change and snapshot a matching subscription is owed. The single
//! reader serves both the single-entity and the batch read ports, so the
//! whole-entity and attribute-level paths share one fixture.

use crate::{
    EntityService,
    NotificationWriteConfig,
    batch::execution::BatchExecutionConfig,
    building_subscription_fixture::{
        BUILDING_TYPE_IRI,
        building_subscription_cache,
        entity_updated_building_subscription_cache,
        subscription_key,
        twin_building_subscription_cache,
    },
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    change_sink::EntityChangeSink,
    configured_policy::payload_budget,
    deletion_outline::DeletionWatchers,
    deletion_outline_fixture::{PayloadOutline, ProbeAnswer},
    port::{
        attribute_history::AttributeDatasetHistory,
        blocking_executor::BlockingExecutor,
        deletion_outline::DeletionOutlineReader,
        reader::EntityReader,
        service_ports::EntityServicePorts,
    },
    recording_ports::{ImmediateRetry, RecordedCommits, RecordingCommitter, SnapshotReader, UnusedAttributeHistory, UnusedBlockingExecutor},
};
use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    committer::ContextCommitter,
    context_change_event::ContextChangeEvent,
    entity_version::EntityVersion,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, reads::attribute_change::AttributeChangeKind};
use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::filter::language_matching::LanguageMatching;
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;
use andromeda_retry::retry_delay::RetryDelay;
use andromeda_subscription::cache::SubscriptionCache;
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{
    collections::HashSet,
    num::NonZeroUsize,
    sync::{Arc, Mutex, atomic::AtomicUsize},
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

/// The Building the reader returns, carrying one `category` Property so the
/// update, partial-update, delete and replace operations have an attribute to
/// act on.
pub(crate) fn building_snapshot() -> EntityState {
    let stored = json!([{
        "@id": ENTITY_ID,
        "@type": [BUILDING_TYPE_IRI],
        CATEGORY_IRI: [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "office"}],
            "https://uri.etsi.org/ngsi-ld/createdAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T10:00:00.000000Z"}],
            "https://uri.etsi.org/ngsi-ld/modifiedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T10:00:00.000000Z"}],
            ACCURACY_IRI: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "high"}],
                "https://uri.etsi.org/ngsi-ld/createdAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T10:00:00.000000Z"}],
                "https://uri.etsi.org/ngsi-ld/modifiedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T10:00:00.000000Z"}]
            }]
        }]
    }]);
    EntityState::from_slice(&serde_json::to_vec(&stored).expect("the fixture is JSON")).expect("the fixture is a stored entity")
}

/// An entity service of a durable-delivery broker wired to the recording
/// committer, with the one Building subscription in its cache, returning the
/// shared recording buffer.
pub(crate) async fn recording_service() -> (EntityService, Arc<Mutex<Vec<AtomicWrite>>>) {
    let (service, RecordedCommits { writes: recorded, .. }, _payload_reads) = recording_service_with(
        ChangeNotifier::new(
            None,
            SubscriptionMatching::AtCommit(NotificationWriteConfig {
                subscription_cache: building_subscription_cache().await,
            }),
        ),
        ProbeAnswer::Answers(DeletionWatchers::Possible),
        payload_budget(&RequestLimitsConfig::default()),
    );
    (service, recorded)
}

/// [`recording_service`], also counting the commits that arrive as
/// respond-async ones.
pub(crate) async fn recording_service_counting_enqueued() -> (EntityService, RecordedCommits) {
    let (service, recorded, _payload_reads) = recording_service_with(
        ChangeNotifier::new(
            None,
            SubscriptionMatching::AtCommit(NotificationWriteConfig {
                subscription_cache: building_subscription_cache().await,
            }),
        ),
        ProbeAnswer::Answers(DeletionWatchers::Possible),
        payload_budget(&RequestLimitsConfig::default()),
    );
    (service, recorded)
}

/// The default payload budget with each geometry held to `positions`
/// positions.
pub(crate) fn geometry_budget(positions: usize) -> EntityPayloadBudget {
    EntityPayloadBudget {
        geometry_positions: GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("the fixture budget admits a position")),
        ..payload_budget(&RequestLimitsConfig::default())
    }
}

/// An entity service of a live-delivery broker admitting payloads within
/// `budget`.
pub(crate) fn budgeted_recording_service(budget: EntityPayloadBudget) -> EntityService {
    let (service, _recorded, _payload_reads) = recording_service_with(
        ChangeNotifier::new(None, SubscriptionMatching::Absent),
        ProbeAnswer::Answers(DeletionWatchers::Possible),
        budget,
    );
    service
}

/// An entity service under observation: what it committed, how many whole
/// states it read, and every change it published.
pub(crate) struct ObservedService {
    /// The service under test.
    pub(crate) service: EntityService,
    /// The writes it committed.
    pub(crate) recorded: Arc<Mutex<Vec<AtomicWrite>>>,
    /// How many whole states it read.
    pub(crate) payload_reads: Arc<AtomicUsize>,
    /// The changes it published.
    pub(crate) published: Arc<Mutex<Vec<ChangeEvent>>>,
}

/// A change sink keeping every change it is handed.
pub(crate) struct RecordingSink {
    /// The changes handed to the sink, in order.
    pub(crate) published: Arc<Mutex<Vec<ChangeEvent>>>,
}

impl EntityChangeSink for RecordingSink {
    fn publish(&self, event: ContextChangeEvent) {
        self.published.lock().expect("recording mutex is not poisoned").push(event.event);
    }
}

/// An entity service of a durable-delivery broker whose deletion-watcher probe
/// answers `probe`, with the subscriptions `cached` names in its cache.
pub(crate) async fn probing_recording_service(probe: ProbeAnswer, cached: CachedSubscription) -> ObservedService {
    let subscription_cache = cache_holding(cached).await;
    observed_service(probe, SubscriptionMatching::AtCommit(NotificationWriteConfig { subscription_cache }))
}

/// An entity service of a live-delivery broker whose deletion-watcher probe
/// answers `probe`, with the subscriptions `cached` names in its cache.
pub(crate) async fn probing_live_service(probe: ProbeAnswer, cached: CachedSubscription) -> ObservedService {
    observed_service(probe, SubscriptionMatching::AtDelivery(cache_holding(cached).await))
}

/// A subscription cache holding what `cached` names.
async fn cache_holding(cached: CachedSubscription) -> Arc<SubscriptionCache> {
    match cached {
        CachedSubscription::Building => building_subscription_cache().await,
        CachedSubscription::BuildingTwice => twin_building_subscription_cache().await,
        CachedSubscription::BuildingOnEntityUpdated => entity_updated_building_subscription_cache().await,
        CachedSubscription::None => Arc::new(SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality)),
    }
}

/// An entity service matching as `matching` says, whose deletion-watcher
/// probe answers `probe`, observed through what it commits, reads and
/// publishes.
fn observed_service(probe: ProbeAnswer, matching: SubscriptionMatching) -> ObservedService {
    let published = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingSink {
        published: Arc::clone(&published), // clone: the sink and the test both hold the published changes
    };
    let notifier = ChangeNotifier::new(Some(Arc::new(sink) as Arc<dyn EntityChangeSink>), matching);
    let (service, RecordedCommits { writes: recorded, .. }, payload_reads) =
        recording_service_with(notifier, probe, payload_budget(&RequestLimitsConfig::default()));
    ObservedService {
        service,
        recorded,
        payload_reads,
        published,
    }
}

/// Which subscription a probing service's cache holds.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CachedSubscription {
    /// The one Building subscription.
    Building,
    /// Two Building subscriptions, each matching every change of the
    /// Building.
    BuildingTwice,
    /// One Building subscription listening for `entityUpdated` alone.
    BuildingOnEntityUpdated,
    /// None at all, as on a replica that loaded the tenant before another
    /// replica created the subscription.
    None,
}

/// An entity service of a live-delivery broker wired to the recording
/// committer: it records no durable match, because its notification worker
/// evaluates every change in-process.
pub(crate) fn live_recording_service() -> (EntityService, Arc<Mutex<Vec<AtomicWrite>>>) {
    let (service, RecordedCommits { writes: recorded, .. }, _payload_reads) = recording_service_with(
        ChangeNotifier::new(None, SubscriptionMatching::Absent),
        ProbeAnswer::Answers(DeletionWatchers::Possible),
        payload_budget(&RequestLimitsConfig::default()),
    );
    (service, recorded)
}

/// An entity service over the recording committer that notifies through
/// `notifier` and admits payloads within `budget`.
fn recording_service_with(notifier: ChangeNotifier, probe: ProbeAnswer, budget: EntityPayloadBudget) -> (EntityService, RecordedCommits, Arc<AtomicUsize>) {
    let (committer, recorded) = RecordingCommitter::new();
    let payload_reads = Arc::new(AtomicUsize::new(0));
    let modified_at = Utc::now();
    let version = EntityVersion::new(1, 1);
    let building = Arc::new(building_snapshot());
    let service = EntityService::new(
        EntityServicePorts {
            reader: Arc::new(SnapshotReader {
                state: Arc::clone(&building), // clone: the reader and the outline serve the one fixture
                modified_at,
                version,
                payload_reads: Arc::clone(&payload_reads), // clone: the test reads the count the reader keeps
            }) as Arc<dyn EntityReader>,
            blocking_executor: Arc::new(UnusedBlockingExecutor) as Arc<dyn BlockingExecutor>,
            retry_delay: Arc::new(ImmediateRetry) as Arc<dyn RetryDelay>,
            attribute_history: Arc::new(UnusedAttributeHistory) as Arc<dyn AttributeDatasetHistory>,
            writes: Arc::new(committer) as Arc<dyn ContextCommitter>,
            deletion_outline: Arc::new(PayloadOutline {
                state: building,
                version,
                probe,
            }) as Arc<dyn DeletionOutlineReader>,
        },
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline JSON-LD engine builds"),
        Arc::new(notifier),
        BatchExecutionConfig {
            chunk_concurrency: 1,
            chunk_write_concurrency: 1,
            chunk_size: 1,
            large_threshold: 1,
            // Above any batch these tests submit, so preparation stays inline on
            // the calling task rather than offloading to the stub blocking pool.
            prepare_blocking_threshold: 1024,
            async_side_effect_threshold: None,
        },
        budget,
    );
    (service, recorded, payload_reads)
}

/// The ordinary branch target used by the recording fixtures.
pub(crate) fn live_target() -> ContextTarget {
    ContextTarget::Live
}

/// The fixture entity identifier as a [`Identifier`].
pub(crate) fn entity_uri() -> Identifier {
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

/// Assert the write is owed exactly one match against the fixture
/// subscription, carrying the post-mutation snapshot and exactly the given
/// `(transition, attribute)` instances as an order-independent set: one change
/// is one notification however many instances it touched (clause 5.8.6), and
/// the order of its instances is not spec-significant.
pub(crate) fn assert_single_match(write: &AtomicWrite, expected: &[(AttributeChangeKind, &str)]) {
    assert_eq!(
        write.subscription_matches.len(),
        1,
        "the matching subscription is owed exactly one notification"
    );
    let matched = &write.subscription_matches[0];
    assert_eq!(matched.subscription_key, subscription_key());
    assert_eq!(matched.entity_id.as_str(), ENTITY_ID);
    assert!(matched.entity_snapshot.is_some(), "the durable match carries the post-mutation entity snapshot");
    let actual: HashSet<(AttributeChangeKind, &AttributeName)> = matched.change.instances.iter().map(|instance| (instance.kind, &instance.key.attr)).collect();
    let names: Vec<(AttributeChangeKind, AttributeName)> = expected.iter().map(|(kind, changed)| (*kind, attribute_name(changed))).collect();
    let expected: HashSet<(AttributeChangeKind, &AttributeName)> = names.iter().map(|(kind, name)| (*kind, name)).collect();
    assert_eq!(actual, expected);
}

/// Assert the write is owed one match for each of the two Building
/// subscriptions, sharing one notified state and one previous state: the
/// states a change is notified with are built once, however many
/// subscriptions it matched.
pub(crate) fn assert_matches_share_their_snapshots(write: &AtomicWrite) {
    let [first, second] = write.subscription_matches.as_slice() else {
        panic!("the change matches both Building subscriptions: {:?}", write.subscription_matches);
    };
    let (Some(first_state), Some(second_state)) = (&first.entity_snapshot, &second.entity_snapshot) else {
        panic!("both matches carry the notified state");
    };
    assert!(Arc::ptr_eq(first_state, second_state), "the matches share one notified state");
    let (Some(first_previous), Some(second_previous)) = (&first.previous_snapshot, &second.previous_snapshot) else {
        panic!("both matches carry the previous state");
    };
    assert!(Arc::ptr_eq(first_previous, second_previous), "the matches share one previous state");
}
