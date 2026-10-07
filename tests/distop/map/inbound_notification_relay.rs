//! What a front broker does with a notification a context source posts to it under the identifier of a forwarded subscription copy (clauses 5.8.6 and 6.3.4). A stub Context Source stands in for the sender, accepts the copy and shows the copy's identifier; the test then posts the crafted notification at the front broker's receiver as the source would, and reads what the local subscriber is sent.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, relationship},
    map::{
        notification_inspection::notified_entities,
        subscription_document::{set_entity_type, subscription},
    },
    notification_adapter::NotificationAdapter,
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::{Map, Value, json};
use std::time::Duration;

/// Delivery poll budget for a relayed notification.
const DELIVER: Duration = Duration::from_secs(12);

/// How long to wait to be sure a notification is not coming.
const ABSENT: Duration = Duration::from_secs(3);

/// The resource a forwarded subscription copy is created on at a source.
const FORWARDED_SUBSCRIPTIONS_PATH: &str = "/ngsi-ld/v1/subscriptions";

/// The resource a source notifies the front broker's forwarded copy at.
const RECEIVER_PATH: &str = "/ngsi-ld/v1/notifications";

/// One front broker, the stub source it forwarded a subscription copy to, and the local subscriber of that subscription.
struct RelayScene {
    /// The single-broker topology the scene runs on.
    topology: MultiBrokerTopology<BrokerRole>,
    /// The local subscriber's endpoint.
    receiver: NotificationAdapter,
    /// The stub source standing in for the sender of the inbound notifications.
    source: ContextSourceAdapter,
    /// The identifier of the copy the source holds, which it notifies under.
    copy_id: String,
    /// The per-test entity type the subscription watches.
    entity_type: String,
    /// The local subscription.
    subscription_id: String,
}

impl RelayScene {
    /// A front broker with the stub source registered for a per-test entity type and a local subscription of that type created from `subscription(..)` after `customize` has changed it.
    async fn start(label: &str, customize: impl FnOnce(&mut Value)) -> RelayScene {
        let topology = topology::single().await.expect("single-broker topology");
        let receiver = NotificationAdapter::start().await;
        let source = ContextSourceAdapter::start().await;
        source.set_stub(Method::POST, FORWARDED_SUBSCRIPTIONS_PATH, 201).await;
        let entity_type = topology.id("Vehicle");
        let subscription_id = topology.id(&format!("Subscription:{label}"));
        let front = topology.broker(BrokerRole::A).expect("topology broker A").client();
        let registration = CsrBuilder::new(&topology.id(&format!("registration:{label}")), &source.url("/"))
            .tenant(topology.tenant())
            .mode("inclusive")
            .entity_type(&entity_type)
            .operations(&["federationOps"])
            .build();
        assert_status(&front.create_registration(&registration).await.expect("register source"), 201);
        let mut local = subscription(&subscription_id, &receiver.endpoint_url());
        set_entity_type(&mut local, &entity_type);
        customize(&mut local);
        assert_status(&front.create_subscription(&local).await.expect("create subscription"), 201);
        let copy_id = placed_copy_id(&source).await;
        RelayScene {
            topology,
            receiver,
            source,
            copy_id,
            entity_type,
            subscription_id,
        }
    }

    /// The entity id `name` names in this scene.
    fn entity_id(&self, name: &str) -> String {
        self.topology.id(&format!("Vehicle:{name}"))
    }

    /// Post `notification` at the front broker's receiver as the source would,
    /// with `content_type`.
    async fn post(&self, content_type: &str, notification: &Value) -> TestResponse {
        self.topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .raw(Method::POST, RECEIVER_PATH)
            .header("Content-Type", content_type)
            .json(notification)
            .send()
            .await
            .expect("notify the front broker")
    }

    /// The `application/ld+json` notification of `data` under the copy.
    fn inbound(&self, data: &Value) -> Value {
        json!({
            "id": "urn:ngsi-ld:Notification:inbound",
            "type": "Notification",
            "subscriptionId": self.copy_id,
            "notifiedAt": "2026-10-06T12:00:00Z",
            "data": data,
            "@context": COMPOUND_CONTEXT.as_str()
        })
    }

    /// Wait for the notification the local subscriber is sent.
    async fn relayed(&self) -> Vec<Value> {
        notified_entities(&self.receiver.wait_for_subscription_notification(&self.subscription_id, DELIVER).await)
    }

    /// Release the scene's servers and the topology.
    async fn finish(self) {
        self.receiver.shutdown().await;
        self.source.shutdown().await;
        self.topology.finish().await.expect("topology cleanup");
    }
}

/// The identifier of the subscription copy the front broker placed at `source`: the `id` of the first copy created there.
async fn placed_copy_id(source: &ContextSourceAdapter) -> String {
    loop {
        let request = source.next_request(DELIVER).await;
        if request.method == Method::POST && request.uri.path() == FORWARDED_SUBSCRIPTIONS_PATH {
            let copy: Value = request.json().expect("the copy is a JSON subscription");
            return copy["id"].as_str().expect("the copy names its identifier").to_owned();
        }
    }
}

/// The member names an entity carries.
fn members(entity: &Value) -> Vec<&str> {
    let mut names: Vec<&str> = entity.as_object().map(Map::keys).into_iter().flatten().map(String::as_str).collect();
    names.sort_unstable();
    names
}

/// The entity of `entities` named `id`.
fn entity_named<'a>(entities: &'a [Value], id: &str) -> &'a Value {
    entities.iter().find(|entity| entity["id"].as_str() == Some(id)).unwrap_or(&Value::Null)
}

/// Clause 5.8.6 with JSON-LD 1.1: an `application/ld+json` notification, the shape this broker itself sends to an `application/ld+json` endpoint, writes the `@context` on each entity of `data`, and a node's context applies to that node.
#[tokio::test]
async fn an_ld_json_notification_with_entity_contexts_is_relayed() {
    let scene = RelayScene::start("relay-entity-contexts", |_| {}).await;
    let first = scene.entity_id("contexts-1");
    let second = scene.entity_id("contexts-2");
    let data = json!([
        entity(&first, &scene.entity_type, &[("speed", property(json!(40)))]),
        entity(&second, &scene.entity_type, &[("speed", property(json!(50)))])
    ]);

    let answer = scene.post("application/ld+json", &scene.inbound(&data)).await;
    let relayed = scene.relayed().await;
    scene.finish().await;

    assert_status(&answer, 200);
    assert_eq!(entity_named(&relayed, &first)["speed"], json!({"type": "Property", "value": 40}), "{relayed:?}");
    assert_eq!(
        entity_named(&relayed, &second)["speed"],
        json!({"type": "Property", "value": 50}),
        "{relayed:?}"
    );
}

/// Clause 5.8.6: an entity that cannot be read leaves the copy and the others are relayed, and the sender is answered as for any notification it posted.
#[tokio::test]
async fn one_invalid_entity_among_valid_ones_is_skipped() {
    let scene = RelayScene::start("relay-one-invalid", |_| {}).await;
    let first = scene.entity_id("invalid-1");
    let typeless = scene.entity_id("invalid-2");
    let third = scene.entity_id("invalid-3");
    let data = json!([
        entity(&first, &scene.entity_type, &[("speed", property(json!(40)))]),
        {"id": typeless, "speed": property(json!(41))},
        entity(&third, &scene.entity_type, &[("speed", property(json!(42)))])
    ]);

    let answer = scene.post("application/ld+json", &scene.inbound(&data)).await;
    let relayed = scene.relayed().await;
    scene.finish().await;

    assert_status(&answer, 200);
    let mut ids: Vec<&str> = relayed.iter().filter_map(|entity| entity["id"].as_str()).collect();
    ids.sort_unstable();
    let mut expected = vec![first.as_str(), third.as_str()];
    expected.sort_unstable();
    assert_eq!(ids, expected, "{relayed:?}");
}

/// Clause 6.3.4: `application/merge-patch+json` is the media type of the Merge Entity operation alone, so a notification posted as one is refused with 415 and nothing is relayed.
#[tokio::test]
async fn merge_patch_notification_is_refused() {
    let scene = RelayScene::start("relay-merge-patch", |_| {}).await;
    let entity_id = scene.entity_id("merge-patch");
    let data = json!([entity(&entity_id, &scene.entity_type, &[("speed", property(json!(40)))])]);

    let answer = scene.post("application/merge-patch+json", &scene.inbound(&data)).await;
    scene.receiver.wait_for_no_subscription_notification(&scene.subscription_id, ABSENT).await;
    scene.finish().await;

    assert_status(&answer, 415);
}

/// Clause 5.8.6 with clauses 4.21 and 5.3.1: a notification a source relays is written the way the local subscription writes a local entity, so the same subscription gives the same members and the same spelling for an entity held locally and for one a source notified. `pick` removes `type`, keeps `id`, and a Relationship target is spelled decoded.
#[tokio::test]
async fn relayed_notification_matches_a_local_one() {
    let scene = RelayScene::start("relay-matches-local", |local| {
        local["notification"]["attributes"] = json!(["speed", "owner"]);
    })
    .await;
    let local_id = scene.entity_id("matches-local");
    let remote_id = scene.entity_id("matches-remote");
    let owner = "urn:ngsi-ld:Person:ada%3Alovelace";
    let front = scene.topology.broker(BrokerRole::A).expect("topology broker A").client();
    assert_status(
        &front
            .post_entity(&entity(
                &local_id,
                &scene.entity_type,
                &[("speed", property(json!(40))), ("owner", relationship(owner))],
            ))
            .with_ld_json()
            .send()
            .await
            .expect("create the local entity"),
        201,
    );
    let local = scene.relayed().await;
    let data = json!([entity(
        &remote_id,
        &scene.entity_type,
        &[("speed", property(json!(40))), ("owner", relationship(owner))]
    )]);

    let answer = scene.post("application/ld+json", &scene.inbound(&data)).await;
    let relayed = scene.relayed().await;
    scene.finish().await;

    assert_status(&answer, 200);
    let local = entity_named(&local, &local_id);
    let relayed = entity_named(&relayed, &remote_id);
    assert_eq!(members(relayed), members(local), "local: {local}\nrelayed: {relayed}");
    assert_eq!(relayed["speed"], local["speed"]);
    assert_eq!(relayed["owner"]["object"], local["owner"]["object"], "local: {local}\nrelayed: {relayed}");
}

/// Table 5.2.12-1 `lang` and clause 4.15: the subscription's `lang` picks the language of every `LanguageProperty` of a notified entity, whether the entity is held locally or was relayed from a source.
#[tokio::test]
async fn relayed_notification_applies_the_subscription_lang() {
    let scene = RelayScene::start("relay-applies-lang", |local| {
        local["lang"] = json!("en");
    })
    .await;
    let remote_id = scene.entity_id("applies-lang");
    let data = json!([entity(
        &remote_id,
        &scene.entity_type,
        &[("name", json!({"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}}))]
    )]);

    let answer = scene.post("application/ld+json", &scene.inbound(&data)).await;
    let relayed = scene.relayed().await;
    scene.finish().await;

    assert_status(&answer, 200);
    let name = &entity_named(&relayed, &remote_id)["name"];
    assert_eq!(name["type"], json!("Property"), "{name}");
    assert_eq!(name["value"], json!("Tower"), "{name}");
    assert_eq!(name["lang"], json!("en"), "{name}");
}

/// Clause 5.8.6: a subscription declaring `ngsildConformance` has every Entity it sends amended to that version, a relayed one included (Tables 4.3.6.8-1 and 4.3.6.8-2: `observedAt` is 1.3 and the `LanguageProperty` 1.4, both unknown to 1.0).
#[tokio::test]
async fn relayed_notification_honours_conformance() {
    let scene = RelayScene::start("relay-conformance", |local| {
        local["ngsildConformance"] = json!("1.0");
    })
    .await;
    let remote_id = scene.entity_id("conformance");
    let data = json!([entity(
        &remote_id,
        &scene.entity_type,
        &[
            ("speed", json!({"type": "Property", "value": 20, "observedAt": "2025-01-01T00:00:00Z"})),
            ("name", json!({"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}}))
        ]
    )]);

    let answer = scene.post("application/ld+json", &scene.inbound(&data)).await;
    let relayed = scene.relayed().await;
    scene.finish().await;

    assert_status(&answer, 200);
    let amended = entity_named(&relayed, &remote_id);
    assert!(amended["speed"].get("observedAt").is_none(), "{amended}");
    assert_eq!(amended["name"]["type"], json!("Property"), "{amended}");
}

/// Clause 5.8.6: a source reports a deleted attribute as "only the name of the attribute as key and the URI "urn:ngsi-ld:null" as value", and the copy relayed in the `format` of the local subscription reports it the same way.
async fn assert_deletion_relayed_bare(label: &str, format: &str) {
    let scene = RelayScene::start(label, |local| {
        local["notification"]["format"] = json!(format);
    })
    .await;
    let entity_id = scene.entity_id("deletion");
    let data = json!([entity(
        &entity_id,
        &scene.entity_type,
        &[("owner", json!("urn:ngsi-ld:null")), ("speed", property(json!(40)))]
    )]);

    let answer = scene.post("application/ld+json", &scene.inbound(&data)).await;
    let relayed = scene.relayed().await;
    scene.finish().await;

    assert_status(&answer, 200);
    let notified = entity_named(&relayed, &entity_id);
    assert_eq!(notified["owner"], json!("urn:ngsi-ld:null"), "{format}: {notified}");
}

/// A relayed deletion is the bare Null in the concise representation.
#[tokio::test]
async fn regression_r56_a_notified_deletion_is_relayed_bare_in_the_concise_format() {
    assert_deletion_relayed_bare("relay-deletion-concise", "concise").await;
}

/// A relayed deletion is the bare Null in the simplified (`keyValues`) representation.
#[tokio::test]
async fn regression_r56_a_notified_deletion_is_relayed_bare_in_the_simplified_format() {
    assert_deletion_relayed_bare("relay-deletion-simplified", "keyValues").await;
}

/// Clause 5.8.6 with clause 4.5.5: when the local subscription splits entities, the part a source notified is completed from this broker's own copy, and an attribute the source notified deleted stays the bare Null of that notification while no other holder's instance of it overrides the deletion. The part keeps what the source named, and the local copy fills in what the source did not.
#[tokio::test]
async fn regression_r56_a_notified_deletion_survives_split_reassembly() {
    let scene = RelayScene::start("relay-deletion-split", |local| {
        local["splitEntities"] = json!(true);
    })
    .await;
    let entity_id = scene.entity_id("deletion-split");
    let front = scene.topology.broker(BrokerRole::A).expect("topology broker A").client();
    assert_status(
        &front
            .post_entity(&entity(
                &entity_id,
                &scene.entity_type,
                &[("owner", relationship("urn:ngsi-ld:Person:held-locally")), ("colour", property(json!("red")))],
            ))
            .with_ld_json()
            .send()
            .await
            .expect("create the local copy"),
        201,
    );
    scene.receiver.wait_for_subscription_notification(&scene.subscription_id, DELIVER).await;
    let data = json!([entity(
        &entity_id,
        &scene.entity_type,
        &[("owner", json!("urn:ngsi-ld:null")), ("speed", property(json!(40)))]
    )]);

    let answer = scene.post("application/ld+json", &scene.inbound(&data)).await;
    let relayed = scene.relayed().await;
    scene.finish().await;

    assert_status(&answer, 200);
    let completed = entity_named(&relayed, &entity_id);
    assert_eq!(completed["owner"], json!("urn:ngsi-ld:null"), "{completed}");
    assert_eq!(completed["speed"]["value"], json!(40), "{completed}");
    assert_eq!(completed["colour"]["value"], json!("red"), "the local holder completes the part: {completed}");
}

/// The entities the local subscriber is sent for a relayed Vehicle located in a City the front broker holds, under a subscription naming `join`, with the City's identifier.
async fn relay_with_join(label: &str, join: &str) -> (Vec<Value>, String, String) {
    let scene = RelayScene::start(label, |local| {
        local["notification"]["join"] = json!(join);
    })
    .await;
    let city_id = scene.topology.id(&format!("City:{label}"));
    let vehicle_id = scene.entity_id(label);
    let front = scene.topology.broker(BrokerRole::A).expect("topology broker A").client();
    let city = entity(&city_id, &scene.topology.id("City"), &[("name", property(json!("Paris")))]);
    assert_status(&front.post_entity(&city).with_ld_json().send().await.expect("create the linked entity"), 201);
    let data = json!([entity(&vehicle_id, &scene.entity_type, &[("locatedIn", relationship(&city_id))])]);

    let answer = scene.post("application/ld+json", &scene.inbound(&data)).await;
    let relayed = scene.relayed().await;
    scene.finish().await;

    assert_status(&answer, 200);
    (relayed, vehicle_id, city_id)
}

/// Clause 4.5.23.2 with clause 5.8.6: a subscription with `join=inline` sends a relayed entity as it sends a local one, the Linked Entity the front broker holds nested under the Relationship naming it.
#[tokio::test]
async fn relayed_notification_applies_an_inline_join() {
    let (relayed, vehicle_id, city_id) = relay_with_join("relay-join-inline", "inline").await;

    let vehicle = entity_named(&relayed, &vehicle_id);
    assert_eq!(relayed.len(), 1, "{relayed:?}");
    assert_eq!(vehicle["locatedIn"]["entity"]["id"], json!(city_id), "{vehicle}");
    assert_eq!(vehicle["locatedIn"]["entity"]["name"]["value"], json!("Paris"), "{vehicle}");
}

/// Clause 4.5.23.3 with clause 5.8.6: a subscription with `join=flat` sends the Linked Entity beside a relayed entity, as it does beside a local one.
#[tokio::test]
async fn relayed_notification_applies_a_flat_join() {
    let (relayed, vehicle_id, city_id) = relay_with_join("relay-join-flat", "flat").await;

    assert_eq!(relayed.len(), 2, "{relayed:?}");
    assert_eq!(entity_named(&relayed, &city_id)["name"]["value"], json!("Paris"), "{relayed:?}");
    assert!(entity_named(&relayed, &vehicle_id)["locatedIn"].get("entity").is_none(), "{relayed:?}");
}
