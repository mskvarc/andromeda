//! A Building store another writer changes while an operation is under way,
//! for the tests proving each read-modify-write states the version it read.
//!
//! The store is the in-memory one, which applies a write's precondition the
//! way the PostgreSQL committer does; the concurrent writer runs between the
//! operation's read and its commit, which is where another broker replica's
//! write lands.

use crate::{
    EntityService,
    NotificationWriteConfig,
    admission_fixture::admitted_batch,
    batch::{BatchResult, UpsertPolicy},
    building_subscription_fixture::{BUILDING_TYPE_IRI, building_subscription_cache},
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    change_sink::EntityChangeSink,
    in_memory_rows::StoredRows,
    in_memory_service::in_memory_service,
    in_memory_store::{ConcurrentWriter, InMemoryStore},
    notification_recording_harness::RecordingSink,
    side_effect::SideEffectPlacement,
    write_kind::{BatchMerge, BatchUpsert},
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    committer::ContextCommitter,
    entity_version::EntityVersion,
    history_record::HistoryRecord,
    precondition::Precondition,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, instance::Instance};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember},
    entity::Entity,
};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};

/// The vocabulary the fixture's terms expand under.
const DEFAULT_VOCABULARY: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// The expanded IRI of the term `name`.
pub(crate) fn term(name: &str) -> String {
    format!("{DEFAULT_VOCABULARY}{name}")
}

/// The identifier of the Building named `name`.
pub(crate) fn building_id(name: &str) -> Identifier {
    format!("urn:ngsi-ld:Building:{name}").parse().expect("a valid identifier")
}

/// One expanded Property instance holding `value`, carrying the clause 4.8
/// `createdAt` and `modifiedAt` every stored instance has.
fn property_instance(value: i64) -> Value {
    let stamp = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T10:00:00.000000Z"}]);
    json!({
        "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
        "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}],
        "https://uri.etsi.org/ngsi-ld/createdAt": stamp,
        "https://uri.etsi.org/ngsi-ld/modifiedAt": stamp
    })
}

/// The stored Building `id` holding a Property of `value` for each of
/// `attributes`.
fn building_with(id: &Identifier, attributes: &[&str], value: i64) -> EntityState {
    let mut payload = Map::new();
    payload.insert("@id".to_owned(), json!(id.as_str()));
    payload.insert("@type".to_owned(), json!([BUILDING_TYPE_IRI]));
    for attribute in attributes {
        payload.insert(term(attribute), json!([property_instance(value)]));
    }
    EntityState::from_slice(&serde_json::to_vec(&Value::Array(vec![Value::Object(payload)])).expect("JSON")).expect("the fixture is a stored entity")
}

/// The stored Building `id` holding a Property for each of `attributes`.
pub(crate) fn stored_building(id: &Identifier, attributes: &[&str]) -> EntityState {
    building_with(id, attributes, 1)
}

/// The request Building `id`, in compact form, carrying `attributes`.
pub(crate) fn request_building<M: MemberReading>(id: &Identifier, attributes: &Value) -> Entity<M> {
    let mut entity = json!({"id": id.as_str(), "type": "Building"});
    if let (Some(entity), Some(attributes)) = (entity.as_object_mut(), attributes.as_object()) {
        // clone: the fixture's attributes are borrowed and the entity owns its members
        entity.extend(attributes.iter().map(|(name, member)| (name.clone(), member.clone())));
    }
    serde_json::from_value(entity).expect("the fixture entity is valid")
}

/// The attribute terms a stored entity carries, sorted.
pub(crate) fn attribute_terms(state: &EntityState) -> Vec<String> {
    let mut terms: Vec<String> = state
        .attributes()
        .names()
        .filter_map(|name| name.as_str().strip_prefix(DEFAULT_VOCABULARY))
        .map(str::to_owned)
        .collect();
    terms.sort_unstable();
    terms
}

/// The attribute terms the state `write` stores carries.
pub(crate) fn written_terms(write: &AtomicWrite) -> Vec<String> {
    attribute_terms(&write.entity.record().expect("the write stores a state").state)
}

/// The attribute terms whose instances `write` records as ended, sorted.
pub(crate) fn tombstoned_terms(write: &AtomicWrite) -> Vec<String> {
    let mut terms: Vec<String> = write
        .history
        .iter()
        .filter_map(|record| match record {
            HistoryRecord::Tombstone(ended) => ended.key.attr.as_str().strip_prefix(DEFAULT_VOCABULARY).map(str::to_owned),
            HistoryRecord::Observations(_) | HistoryRecord::EvolutionInstances(_) | HistoryRecord::ScopeRemoval => None,
        })
        .collect();
    terms.sort_unstable();
    terms
}

/// The version `write` held against.
pub(crate) fn held_version(write: &AtomicWrite) -> EntityVersion {
    match write.precondition {
        Precondition::UnchangedSince(version) => version,
        other @ (Precondition::None | Precondition::MustExist | Precondition::MustNotExist) => {
            panic!("a read-modify-write holds against the version it read, not {other:?}")
        }
    }
}

/// A write that adds the Property `attribute` to the Building `id`, or
/// creates the Building holding it, then hands the rows it left to `record`.
pub(crate) fn attribute_adder(
    id: &Identifier,
    attribute: &str,
    record: impl FnOnce(&StoredRows) + Send + 'static,
) -> impl FnOnce(&mut StoredRows) + Send + 'static {
    let id = id.clone(); // clone: the writer outlives the borrow
    let member: AttributeName = term(attribute).parse().expect("a valid attribute name");
    let addition = building_with(&id, &[attribute], 2);
    move |rows: &mut StoredRows| {
        let mut state = rows.payload(&id).map_or_else(|| stored_building(&id, &[]), |stored| EntityState::clone(stored));
        let added = addition.attributes().get_shared(member.as_str()).expect("the addition holds the attribute");
        state.attributes_mut().insert_shared(member, Arc::clone(added));
        rows.put(&id, Arc::new(state));
        record(rows);
    }
}

/// A writer that adds the Property `attribute` to the Building `id` just
/// before the next commit, and records the version it left the Building at.
pub(crate) fn adds_attribute(id: &Identifier, attribute: &str, version: &Arc<Mutex<Option<EntityVersion>>>) -> ConcurrentWriter {
    let recorded = id.clone(); // clone: the recorder outlives the borrow
    let version = Arc::clone(version);
    ConcurrentWriter::BeforeNextCommit(Box::new(attribute_adder(id, attribute, move |rows: &StoredRows| {
        *version.lock().expect("the version lock is not poisoned") = rows.version(&recorded);
    })))
}

/// Merge `entities` in one batch, inline on the live branch.
pub(crate) async fn batch_merge(contended: &ContendedService, entities: Vec<Entity<MergeMember>>) -> BatchResult {
    let admitted = admitted_batch(&contended.service, BatchMerge, entities, &ContextSpec::core_only()).await;
    contended
        .service
        .batch_merge(SideEffectPlacement::Inline(&ContextTarget::Live), admitted)
        .await
        .expect("the batch completes")
}

/// Upsert the Buildings `entities` in update mode, inline on the live branch.
pub(crate) async fn upsert_update(contended: &ContendedService, entities: Vec<Entity<AttributeValue>>) -> (BatchResult, Vec<Identifier>) {
    let admitted = admitted_batch(&contended.service, BatchUpsert(UpsertPolicy::Update), entities, &ContextSpec::core_only()).await;
    contended
        .service
        .batch_upsert(SideEffectPlacement::Inline(&ContextTarget::Live), admitted)
        .await
        .expect("the batch completes")
}

/// The `modifiedAt` the write stamped on the instance of `attribute`.
pub(crate) fn stamped_modified_at(write: &AtomicWrite, attribute: &str) -> Option<SystemInstant> {
    let state = &write.entity.record().expect("the write stores a state").state;
    state
        .attributes()
        .get(&term(attribute))
        .and_then(|instances| instances.get(None))
        .and_then(Instance::modified_at)
}

/// An entity service over a Building store another writer may change.
pub(crate) struct ContendedService {
    /// The store, which the test seeds, interferes with and inspects.
    pub(crate) store: Arc<InMemoryStore>,
    /// The service under test.
    pub(crate) service: EntityService,
    /// The changes the service published.
    pub(crate) published: Arc<Mutex<Vec<ChangeEvent>>>,
}

/// Which subscriptions the contended service notifies.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Watchers {
    /// None: a deletion reads only its outline.
    None,
    /// The one Building subscription, recorded at commit.
    Building,
}

/// An entity service over an empty Building store, notifying `watchers`.
pub(crate) async fn contended_service(watchers: Watchers) -> ContendedService {
    let store = Arc::new(InMemoryStore::default());
    let published = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingSink {
        published: Arc::clone(&published),
    };
    let matching = match watchers {
        Watchers::None => SubscriptionMatching::Absent,
        Watchers::Building => SubscriptionMatching::AtCommit(NotificationWriteConfig {
            subscription_cache: building_subscription_cache().await,
        }),
    };
    let notifier = ChangeNotifier::new(Some(Arc::new(sink) as Arc<dyn EntityChangeSink>), matching);
    let service = in_memory_service(&store, Arc::clone(&store) as Arc<dyn ContextCommitter>, notifier);
    ContendedService { store, service, published }
}
