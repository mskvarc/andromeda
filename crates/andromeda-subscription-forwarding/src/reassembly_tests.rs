//! What the distributed subscription side does with a notification a source
//! posts under the identifier of the copy it holds (clause 5.8.6), run against
//! a service wired to in-memory doubles.

use crate::reassembly_harness::{
    ReassemblyScene,
    SceneSpec,
    answering_source::AnsweringSource,
    companion_subscription,
    copy_subscription,
    default_iri,
    identifier,
    sender_registration,
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_query::projection::grammar::ProjectionList;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use serde_json::{Map, Value, json};

/// The type of a Property in the stored form of an entity.
const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";

/// The member of a stored Property that holds its value.
const HAS_VALUE: &str = "https://uri.etsi.org/ngsi-ld/hasValue";

/// A `Vehicle` of the entity `id` in the compact form a source notifies it in,
/// holding `attributes`.
fn compact_vehicle(id: &str, attributes: &[(&str, Value)]) -> Value {
    let mut entity = Map::new();
    entity.insert("id".to_owned(), json!(id));
    entity.insert("type".to_owned(), json!("Vehicle"));
    for (name, node) in attributes {
        entity.insert((*name).to_owned(), node.clone()); // clone: the entity owns its members
    }
    Value::Object(entity)
}

/// A Property of `value` in the compact form.
fn property(value: &Value) -> Value {
    json!({"type": "Property", "value": value})
}

/// A `Vehicle` of the entity `id` as this broker stores it, holding Properties
/// of `attributes`.
fn held_vehicle(id: &str, attributes: &[(&str, Value)]) -> EntityState {
    let mut node = Map::new();
    node.insert("@id".to_owned(), json!(id));
    node.insert("@type".to_owned(), json!([default_iri("Vehicle")]));
    for (name, value) in attributes {
        node.insert(default_iri(name), json!([{"@type": [PROPERTY], HAS_VALUE: [{"@value": value}]}]));
    }
    EntityState::from_slice(&serde_json::to_vec(&json!([node])).expect("the entity writes")).expect("a stored entity decodes")
}

/// The notification a source posts under the copy it holds, carrying `data`.
fn notification(data: &Value) -> Value {
    json!({
        "id": "urn:ngsi-ld:Notification:inbound",
        "type": "Notification",
        "subscriptionId": copy_subscription().as_str(),
        "notifiedAt": "2026-10-06T12:00:00Z",
        "data": data
    })
}

/// The entities of the notification the local subscriber was sent, none when it
/// was sent no notification.
async fn relayed(scene: &mut ReassemblyScene) -> Vec<Value> {
    scene.delivered().await.and_then(|sent| sent["data"].as_array().cloned()).unwrap_or_default()
}

/// The identifiers of `entities`, in order.
fn ids(entities: &[Value]) -> Vec<&str> {
    entities.iter().filter_map(|entity| entity["id"].as_str()).collect()
}

/// The stored form of a `pick` or `omit`, one list per element.
fn stored_projection(lists: &[&str]) -> Value {
    let parsed: Vec<ProjectionList> = lists.iter().map(|raw| raw.parse().expect("a valid projection list")).collect();
    serde_json::to_value(parsed).expect("the projection serializes")
}

/// Table 5.3.1-1 and clause 5.8.6: an entity that cannot be read leaves the
/// copy, the other entities of the notification are still relayed, and the
/// subscriber is sent one notification.
#[tokio::test]
async fn a_bad_entity_does_not_stop_the_others() {
    let mut scene = ReassemblyScene::start(SceneSpec::default()).await;
    let data = json!([
        compact_vehicle("urn:ngsi-ld:Vehicle:1", &[("speed", property(&json!(40)))]),
        {"id": "urn:ngsi-ld:Vehicle:2", "speed": property(&json!(41))},
        compact_vehicle("urn:ngsi-ld:Vehicle:3", &[("speed", property(&json!(42)))])
    ]);

    scene.post(&notification(&data)).await.expect("the notification is handled");

    let entities = relayed(&mut scene).await;
    pretty_assertions::assert_eq!(ids(&entities), vec!["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:3"]);
    pretty_assertions::assert_eq!((scene.slots_claimed(), scene.deliveries_succeeded()), (1, 1));
}

/// Clause 5.8.6: with `splitEntities` the part the sender notified is completed
/// from this broker's own copy and from every other holder, never from the
/// sender itself, which already supplied its part.
#[tokio::test]
async fn split_entities_merge_local_and_other_holders() {
    let id = identifier("urn:ngsi-ld:Vehicle:1");
    let sender = AnsweringSource::start(&sender_registration()).await;
    let third = AnsweringSource::start(&identifier("urn:ngsi-ld:ContextSourceRegistration:third")).await;
    third.answer(&id, &compact_vehicle(id.as_str(), &[("temp", property(&json!(21)))])).await;
    let mut scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"splitEntities": true}),
        held: vec![held_vehicle(id.as_str(), &[("name", json!("Roadster"))])],
        registrations: vec![sender.registration(), third.registration()],
        ..SceneSpec::default()
    })
    .await;
    let data = json!([compact_vehicle(id.as_str(), &[("speed", property(&json!(40)))])]);

    scene.post(&notification(&data)).await.expect("the notification is handled");

    let entities = relayed(&mut scene).await;
    let [entity] = entities.as_slice() else {
        panic!("one entity is relayed, got {entities:?}");
    };
    pretty_assertions::assert_eq!(entity["speed"]["value"], json!(40), "the sender's part: {entity}");
    pretty_assertions::assert_eq!(entity["name"]["value"], json!("Roadster"), "this broker's part: {entity}");
    pretty_assertions::assert_eq!(entity["temp"]["value"], json!(21), "the third holder's part: {entity}");
    pretty_assertions::assert_eq!(sender.requests().await, 0, "the sender is not asked for what it just sent");
}

/// Clause 5.8.6: a source reports a deleted attribute as "only the name of the
/// attribute as key and the URI "urn:ngsi-ld:null" as value", and a split part
/// completed from this broker's copy relays the deletion although this broker
/// still holds the attribute.
#[tokio::test]
async fn a_relayed_null_survives_split_reassembly() {
    let id = identifier("urn:ngsi-ld:Vehicle:1");
    let mut scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"splitEntities": true}),
        held: vec![held_vehicle(id.as_str(), &[("owner", json!("Ada")), ("name", json!("Roadster"))])],
        ..SceneSpec::default()
    })
    .await;
    let data = json!([compact_vehicle(
        id.as_str(),
        &[("owner", json!("urn:ngsi-ld:null")), ("speed", property(&json!(40)))]
    )]);

    scene.post(&notification(&data)).await.expect("the notification is handled");

    let entities = relayed(&mut scene).await;
    let [entity] = entities.as_slice() else {
        panic!("one entity is relayed, got {entities:?}");
    };
    pretty_assertions::assert_eq!(entity["owner"], json!("urn:ngsi-ld:null"), "the notified deletion: {entity}");
    pretty_assertions::assert_eq!(entity["name"]["value"], json!("Roadster"), "this broker's part: {entity}");
}

/// Clause 5.8.6: the local subscription's filters are decided on the entity as
/// completed, so an entity whose part alone fails the `q` is relayed when the
/// other holders' members satisfy it, and dropped when they do not.
#[tokio::test]
async fn filters_run_on_the_merged_state() {
    let id = identifier("urn:ngsi-ld:Vehicle:1");
    let q = format!("\"{}\"==\"red\"", default_iri("colour"));
    for (colour, expected) in [("red", vec![id.as_str()]), ("blue", Vec::new())] {
        let mut scene = ReassemblyScene::start(SceneSpec {
            subscription_members: json!({"splitEntities": true, "q": q}),
            held: vec![held_vehicle(id.as_str(), &[("colour", json!(colour))])],
            ..SceneSpec::default()
        })
        .await;
        let data = json!([compact_vehicle(id.as_str(), &[("speed", property(&json!(40)))])]);

        scene.post(&notification(&data)).await.expect("the notification is handled");

        let entities = relayed(&mut scene).await;
        pretty_assertions::assert_eq!(ids(&entities), expected, "the local copy is {colour}");
    }
}

/// Clauses 4.21 and 5.8.6: `pick` narrows a relayed entity as it narrows a
/// local one, so `type` is gone and `id` stays, whatever the list says.
#[tokio::test]
async fn the_structural_projection_applies() {
    let mut scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"notification": {
            "format": "normalized",
            "pick": stored_projection(&["id", &default_iri("speed")])
        }}),
        ..SceneSpec::default()
    })
    .await;
    let data = json!([compact_vehicle(
        "urn:ngsi-ld:Vehicle:1",
        &[("speed", property(&json!(40))), ("colour", property(&json!("red")))]
    )]);

    scene.post(&notification(&data)).await.expect("the notification is handled");

    let entities = relayed(&mut scene).await;
    let [entity] = entities.as_slice() else {
        panic!("one entity is relayed, got {entities:?}");
    };
    let mut members: Vec<&str> = entity.as_object().into_iter().flatten().map(|(name, _)| name.as_str()).collect();
    members.sort_unstable();
    pretty_assertions::assert_eq!(members, vec!["id", "speed"], "{entity}");
}

/// Clause 5.8.6: a relayed entity is written as a local one is, so a
/// Relationship target the source percent-encoded keeps the spelling it was
/// sent with (RFC 3986 section 6.2.2.2).
#[tokio::test]
async fn relayed_targets_keep_their_percent_encoding() {
    let mut scene = ReassemblyScene::start(SceneSpec::default()).await;
    let data = json!([compact_vehicle(
        "urn:ngsi-ld:Vehicle:1",
        &[("owner", json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:a%3Ab"}))]
    )]);

    scene.post(&notification(&data)).await.expect("the notification is handled");

    let entities = relayed(&mut scene).await;
    let [entity] = entities.as_slice() else {
        panic!("one entity is relayed, got {entities:?}");
    };
    pretty_assertions::assert_eq!(entity["owner"]["object"], json!("urn:ngsi-ld:Person:a%3Ab"), "{entity}");
}

/// Clause 5.8.6 and Table 4.3.6.8-1: a subscription declaring
/// `ngsildConformance` has every Entity it sends amended to that version, a
/// relayed one included: below 1.3 an instance has no `observedAt`, and below
/// 1.4 a `LanguageProperty` is a Property.
#[tokio::test]
async fn conformance_applies_through_the_walk() {
    let mut scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"ngsildConformance": "1.0"}),
        ..SceneSpec::default()
    })
    .await;
    let data = json!([compact_vehicle(
        "urn:ngsi-ld:Vehicle:1",
        &[
            ("speed", json!({"type": "Property", "value": 20, "observedAt": "2025-01-01T00:00:00Z"})),
            ("name", json!({"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}}))
        ]
    )]);

    scene.post(&notification(&data)).await.expect("the notification is handled");

    let entities = relayed(&mut scene).await;
    let [entity] = entities.as_slice() else {
        panic!("one entity is relayed, got {entities:?}");
    };
    assert!(entity["speed"].get("observedAt").is_none(), "{entity}");
    pretty_assertions::assert_eq!(entity["name"]["type"], json!("Property"), "{entity}");
}

/// Clause 5.2.12 `throttling`: the slot is claimed only when the copy is ready
/// to send, so a completion that failed before it spent no slot, and the
/// sender's retry is delivered instead of suppressed.
#[tokio::test]
async fn a_failed_completion_keeps_the_throttling_slot() {
    let id = identifier("urn:ngsi-ld:Vehicle:1");
    let mut scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"splitEntities": true, "throttling": 10}),
        held: vec![held_vehicle(id.as_str(), &[("name", json!("Roadster"))])],
        registration_failures: 1,
        ..SceneSpec::default()
    })
    .await;
    let data = json!([compact_vehicle(id.as_str(), &[("speed", property(&json!(40)))])]);

    let first = scene.post(&notification(&data)).await;
    pretty_assertions::assert_eq!((first.is_err(), scene.slots_claimed(), scene.delivered().await.is_none()), (true, 0, true));

    scene.post(&notification(&data)).await.expect("the retry is handled");
    pretty_assertions::assert_eq!(ids(&relayed(&mut scene).await), vec![id.as_str()]);
    pretty_assertions::assert_eq!((scene.slots_claimed(), scene.deliveries_succeeded()), (1, 1));
}

/// A context source registration in the form a companion notification carries it.
fn registration(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": "http://source.example/",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    })
}

/// Clause 5.8.1.4: the copies the local subscription holds are read once for a
/// companion notification, however many registrations it names.
#[tokio::test]
async fn existing_copies_are_loaded_once() {
    let registrations = [
        "urn:ngsi-ld:ContextSourceRegistration:1",
        "urn:ngsi-ld:ContextSourceRegistration:2",
        "urn:ngsi-ld:ContextSourceRegistration:3",
    ];
    let copies = registrations
        .iter()
        .enumerate()
        .map(|(index, registration)| ForwardedCopy {
            registration: identifier(registration),
            remote_subscription: identifier(&format!("urn:ngsi-ld:Subscription:remote-{index}")),
        })
        .collect();
    let scene = ReassemblyScene::start(SceneSpec {
        copies,
        ..SceneSpec::default()
    })
    .await;
    let companion_notification = json!({
        "id": "urn:ngsi-ld:Notification:companion",
        "type": "ContextSourceNotification",
        "subscriptionId": companion_subscription().as_str(),
        "notifiedAt": "2026-10-06T12:00:00Z",
        "triggerReason": "newlyMatching",
        "data": registrations.iter().map(|id| registration(id)).collect::<Vec<_>>()
    });

    scene.post(&companion_notification).await.expect("the notification is handled");

    pretty_assertions::assert_eq!(scene.copies_asked(), 1);
}
