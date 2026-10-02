//! The outline a Delete Entity reads instead of the payload it removes, and
//! the probe deciding whether it may.
//!
//! Clause 5.6.6 removes an entity's current state; its Temporal Evolution
//! records a tombstone per instance it held (clauses 4.5.7 and 4.5.8) and the
//! end of its Scope (clause 4.18). The outline those are written from is read
//! from the rows the write path keeps beside the payload, so each test writes
//! entities over HTTP and reads the outline through the broker's own store,
//! asserting the typed values it hands the deletion. The watcher probe decides
//! whether the removed payload is read at all: only an active, change-driven
//! entity subscription a deletion can raise a trigger for counts (clause
//! 5.2.12, Table 5.2.12-1).

use andromeda_context_commit::scope_presence::ScopePresence;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::deletion_outline::{DeletionOutline, DeletionWatchers};
use andromeda_ngsi_ld_entity::reads::attribute_instance_key::AttributeInstanceKey;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_postgresql_context_store::entity::repo::EntityRepository;
use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_lookup::NameLookup};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::sync::Arc;

/// The core context every body names.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
/// The vocabulary a term the core context does not define expands under.
const DEFAULT_VOCABULARY: &str = "https://uri.etsi.org/ngsi-ld/default-context/";
/// The expanded IRI of `location`, which the core context defines.
const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

/// The broker's own entity store over the private tenant's database.
async fn store(scope: &TenantScopedEnvironment) -> EntityRepository {
    let pool = scope.tenant_database().await.expect("the tenant database exists once an entity was written");
    let attribute_names = Arc::new(NameCache::<AttributeName>::new(&pool)) as Arc<dyn NameLookup<AttributeName>>;
    let entity_type_names = Arc::new(NameCache::<EntityTypeName>::new(&pool)) as Arc<dyn NameLookup<EntityTypeName>>;
    EntityRepository::new(&pool, attribute_names, entity_type_names)
}

/// Create `entity` through `client`.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

/// Create `subscription` through `client`.
async fn subscribe(client: &TestClient, subscription: &Value) {
    let created = client.create_subscription(subscription).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

/// A term of the default vocabulary, expanded.
fn term(local: &str) -> String {
    format!("{DEFAULT_VOCABULARY}{local}")
}

/// The instance identity of `attr` in `dataset_id`, with its family.
fn instance(attr: &str, dataset_id: Option<&str>, family: AttributeType) -> (AttributeInstanceKey, AttributeType) {
    let key = AttributeInstanceKey {
        attr: attr.parse().expect("a valid attribute name"),
        dataset_id: dataset_id.map(|id| id.parse().expect("a valid datasetId")),
    };
    (key, family)
}

/// The outline's instances in a fixed order, the store listing them in its
/// own.
fn sorted(mut instances: Vec<(AttributeInstanceKey, AttributeType)>) -> Vec<(AttributeInstanceKey, AttributeType)> {
    instances.sort_by(|(left, _), (right, _)| {
        left.attr.as_str().cmp(right.attr.as_str()).then_with(|| {
            left.dataset_id
                .as_ref()
                .map(Identifier::as_str)
                .cmp(&right.dataset_id.as_ref().map(Identifier::as_str))
        })
    });
    instances
}

#[tokio::test]
async fn current_projection_a_deletion_outline_lists_every_instance_with_its_dataset_id_and_family() {
    let scope = TenantScopedEnvironment::from_suite("current-projection")
        .await
        .expect("private tenant environment");
    let id = scope.id("Vehicle:outlined");
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": ["Vehicle", "Car"],
            "scope": "/Madrid",
            "speed": [
                {"type": "Property", "value": 80},
                {"type": "Property", "value": 81, "datasetId": "urn:ngsi-ld:Dataset:a"}
            ],
            "owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:1"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [14.5, 46.0]}},
            "@context": CORE_CONTEXT
        }),
    )
    .await;
    let store = store(&scope).await;
    let identifier: Identifier = id.parse().expect("a valid identifier");

    let outline = store
        .deletion_outline(&ContextTarget::Live, &identifier)
        .await
        .expect("the outline reads")
        .expect("the entity exists");

    // The deletion's compare-and-set holds against the version the document
    // row carries, so the outline reads exactly that one.
    let version = store
        .get_snapshot(&ContextTarget::Live, &identifier)
        .await
        .expect("the snapshot reads")
        .expect("the entity exists")
        .modified_at;
    let types: Vec<EntityTypeName> = [term("Car"), term("Vehicle")].iter().map(|name| name.parse().expect("a type name")).collect();
    let expected = DeletionOutline {
        types: types.clone(), // clone: the types read on their own are compared below
        instances: sorted(vec![
            instance(LOCATION, None, AttributeType::GeoProperty),
            instance(&term("owner"), None, AttributeType::Relationship),
            instance(&term("speed"), None, AttributeType::Property),
            instance(&term("speed"), Some("urn:ngsi-ld:Dataset:a"), AttributeType::Property),
        ]),
        scope: ScopePresence::Held,
        modified_at: version,
    };
    assert_eq!(
        DeletionOutline {
            instances: sorted(outline.instances),
            ..outline
        },
        expected
    );
    assert_eq!(
        store.entity_types(&ContextTarget::Live, &identifier).await.expect("the types read"),
        Some(types)
    );
    scope.finish().await.expect("cleanup");
}

#[tokio::test]
async fn current_projection_an_unscoped_entity_outlines_no_scope_and_an_absent_one_no_outline() {
    let scope = TenantScopedEnvironment::from_suite("current-projection")
        .await
        .expect("private tenant environment");
    let id = scope.id("Vehicle:unscoped");
    create(
        scope.client(),
        &json!({"id": id, "type": "Vehicle", "speed": {"type": "Property", "value": 1}, "@context": CORE_CONTEXT}),
    )
    .await;
    let store = store(&scope).await;

    let outline = store
        .deletion_outline(&ContextTarget::Live, &id.parse().expect("a valid identifier"))
        .await
        .expect("the outline reads")
        .expect("the entity exists");
    assert_eq!(outline.scope, ScopePresence::Absent);

    let absent: Identifier = scope.id("Vehicle:absent").parse().expect("a valid identifier");
    assert_eq!(store.deletion_outline(&ContextTarget::Live, &absent).await.expect("the outline reads"), None);
    assert_eq!(store.entity_types(&ContextTarget::Live, &absent).await.expect("the types read"), None);
    scope.finish().await.expect("cleanup");
}

/// Table 5.2.12-1: a subscription with no trigger selection listens for
/// `attributeCreated` and `attributeUpdated`; a periodic one is notified of
/// current state; a paused one of nothing. None of them is owed a deletion,
/// while one listening for `entityUpdated` -- which includes
/// `attributeDeleted` -- is.
#[tokio::test]
async fn current_projection_the_deletion_watcher_probe_counts_only_subscriptions_a_deletion_can_notify() {
    let scope = TenantScopedEnvironment::from_suite("current-projection")
        .await
        .expect("private tenant environment");
    let client = scope.client();
    create(
        client,
        &json!({"id": scope.id("Vehicle:probe"), "type": "Vehicle", "speed": {"type": "Property", "value": 1}, "@context": CORE_CONTEXT}),
    )
    .await;
    let store = store(&scope).await;
    let probe = || store.deletion_watchers(&ContextTarget::Live);
    let endpoint = json!({"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}});
    assert_eq!(probe().await.expect("the probe reads"), DeletionWatchers::None);

    subscribe(
        client,
        &json!({"id": scope.id("Subscription:default"), "type": "Subscription", "entities": [{"type": "Vehicle"}], "notification": endpoint, "@context": CORE_CONTEXT}),
    )
    .await;
    subscribe(
        client,
        &json!({"id": scope.id("Subscription:periodic"), "type": "Subscription", "entities": [{"type": "Vehicle"}], "timeInterval": 3600, "notificationTrigger": ["entityDeleted"], "notification": endpoint, "@context": CORE_CONTEXT}),
    )
    .await;
    subscribe(
        client,
        &json!({"id": scope.id("Subscription:paused"), "type": "Subscription", "entities": [{"type": "Vehicle"}], "isActive": false, "notificationTrigger": ["entityDeleted"], "notification": endpoint, "@context": CORE_CONTEXT}),
    )
    .await;
    assert_eq!(probe().await.expect("the probe reads"), DeletionWatchers::None);

    subscribe(
        client,
        &json!({"id": scope.id("Subscription:updated"), "type": "Subscription", "entities": [{"type": "Vehicle"}], "notificationTrigger": ["entityUpdated"], "notification": endpoint, "@context": CORE_CONTEXT}),
    )
    .await;
    assert_eq!(probe().await.expect("the probe reads"), DeletionWatchers::Possible);
    scope.finish().await.expect("cleanup");
}
