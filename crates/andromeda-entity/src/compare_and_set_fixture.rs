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
    building_subscription_fixture::{BUILDING_TYPE_IRI, building_subscription_cache},
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    change_sink::EntityChangeSink,
    in_memory_service::in_memory_service,
    in_memory_store::{ConcurrentWriter, InMemoryStore, StoredRows},
    notification_recording_harness::RecordingSink,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    committer::ContextCommitter,
    history_record::HistoryRecord,
    precondition::Precondition,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use chrono::{DateTime, Utc};
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

/// One expanded Property instance holding `value`.
fn property_instance(value: i64) -> Value {
    json!({
        "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
        "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
    })
}

/// The stored, expanded Building `id` holding a Property for each of
/// `attributes`.
pub(crate) fn stored_building(id: &Identifier, attributes: &[&str]) -> Value {
    let mut payload = Map::new();
    payload.insert("@id".to_owned(), json!(id.as_str()));
    payload.insert("@type".to_owned(), json!([BUILDING_TYPE_IRI]));
    for attribute in attributes {
        payload.insert(term(attribute), json!([property_instance(1)]));
    }
    Value::Object(payload)
}

/// The request Building `id`, in compact form, carrying `attributes`.
pub(crate) fn request_building(id: &Identifier, attributes: &Value) -> Entity {
    let mut entity = json!({"id": id.as_str(), "type": "Building"});
    if let (Some(entity), Some(attributes)) = (entity.as_object_mut(), attributes.as_object()) {
        // clone: the fixture's attributes are borrowed and the entity owns its members
        entity.extend(attributes.iter().map(|(name, member)| (name.clone(), member.clone())));
    }
    serde_json::from_value(entity).expect("the fixture entity is valid")
}

/// The attribute terms an expanded payload carries, sorted.
pub(crate) fn attribute_terms(payload: &Value) -> Vec<String> {
    let object = match payload {
        Value::Array(nodes) => nodes.first().and_then(Value::as_object),
        Value::Object(object) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    };
    let mut terms: Vec<String> = object
        .into_iter()
        .flat_map(Map::keys)
        .filter_map(|key| key.strip_prefix(DEFAULT_VOCABULARY))
        .map(str::to_owned)
        .collect();
    terms.sort_unstable();
    terms
}

/// The attribute terms the payload `write` stores carries.
pub(crate) fn written_terms(write: &AtomicWrite) -> Vec<String> {
    attribute_terms(&write.entity.record().expect("the write stores a payload").payload)
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
pub(crate) fn held_version(write: &AtomicWrite) -> DateTime<Utc> {
    match write.precondition {
        Precondition::UnchangedSince(version) => version,
        other @ (Precondition::None | Precondition::MustExist | Precondition::MustNotExist) => {
            panic!("a read-modify-write holds against the version it read, not {other:?}")
        }
    }
}

/// A writer that adds the Property `attribute` to the Building `id` just
/// before the next commit, and records the version it left the Building at.
pub(crate) fn adds_attribute(id: &Identifier, attribute: &str, version: &Arc<Mutex<Option<DateTime<Utc>>>>) -> ConcurrentWriter {
    let id = id.clone(); // clone: the writer outlives the borrow
    let member = term(attribute);
    let version = Arc::clone(version); // clone: the test reads the version the writer records
    ConcurrentWriter::BeforeNextCommit(Box::new(move |rows: &mut StoredRows| {
        let mut payload = rows.payload(&id).cloned().unwrap_or_else(|| stored_building(&id, &[])); // clone: the writer stores a changed copy
        if let Some(object) = payload.as_object_mut() {
            object.insert(member, json!([property_instance(2)]));
        }
        rows.put(&id, payload);
        *version.lock().expect("the version lock is not poisoned") = rows.version(&id);
    }))
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
        published: Arc::clone(&published), // clone: the sink and the test both hold the published changes
    };
    let matching = match watchers {
        Watchers::None => SubscriptionMatching::Absent,
        Watchers::Building => SubscriptionMatching::AtCommit(NotificationWriteConfig {
            subscription_cache: building_subscription_cache().await,
        }),
    };
    let notifier = ChangeNotifier::new(Some(Arc::new(sink) as Arc<dyn EntityChangeSink>), matching);
    let service = in_memory_service(&store, Arc::clone(&store) as Arc<dyn ContextCommitter>, notifier); // clone: the service commits to the store the test inspects
    ContendedService { store, service, published }
}
