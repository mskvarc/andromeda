//! What a relayed notification carries of the Linked Entities the local
//! subscription's `join` reaches (clause 4.5.23, clause 5.8.6), run against a
//! service wired to in-memory doubles.

use crate::reassembly_harness::{ReassemblyScene, SceneSpec, copy_subscription, default_iri};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use serde_json::{Value, json};

/// The identifier of the City the relayed Vehicle is located in.
const CITY: &str = "urn:ngsi-ld:City:1";

/// The identifier of the relayed Vehicle.
const VEHICLE: &str = "urn:ngsi-ld:Vehicle:1";

/// A `City` as this broker stores it, holding one Property.
fn held_city() -> EntityState {
    let document = json!([{
        "@id": CITY,
        "@type": [default_iri("City")],
        default_iri("name"): [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "Paris"}]
        }]
    }]);
    EntityState::from_slice(&serde_json::to_vec(&document).expect("the entity writes")).expect("a stored entity decodes")
}

/// The notification a source posts, carrying one Vehicle located in the City.
fn notification() -> Value {
    json!({
        "id": "urn:ngsi-ld:Notification:inbound",
        "type": "Notification",
        "subscriptionId": copy_subscription().as_str(),
        "notifiedAt": "2026-10-06T12:00:00Z",
        "data": [{
            "id": VEHICLE,
            "type": "Vehicle",
            "locatedIn": {"type": "Relationship", "object": CITY}
        }]
    })
}

/// The members a local subscription with `notification_members` stores.
fn subscription_members(notification_members: &Value) -> Value {
    let mut members = json!({"format": "normalized"});
    if let (Some(base), Some(extra)) = (members.as_object_mut(), notification_members.as_object()) {
        base.extend(extra.iter().map(|(name, value)| (name.clone(), value.clone()))); // clone: the stored form owns its members
    }
    json!({"notification": members})
}

/// The entities of the notification the local subscriber was sent after the
/// source posted its notification, with the City held when `held` says so.
async fn relayed_with(notification_members: &Value, held: Vec<EntityState>) -> Vec<Value> {
    let mut scene = ReassemblyScene::start(SceneSpec {
        subscription_members: subscription_members(notification_members),
        held,
        ..SceneSpec::default()
    })
    .await;
    scene.post(&notification()).await.expect("the notification is handled");
    scene.delivered().await.and_then(|sent| sent["data"].as_array().cloned()).unwrap_or_default()
}

/// The identifiers of `entities`, in order.
fn ids(entities: &[Value]) -> Vec<&str> {
    entities.iter().filter_map(|entity| entity["id"].as_str()).collect()
}

/// Clause 4.5.23.2 with clause 5.8.6: a relayed copy of a `join=inline`
/// subscription nests the Linked Entity this broker holds under the
/// Relationship naming it.
#[tokio::test]
async fn an_inline_join_nests_the_held_linked_entity_into_the_relayed_entity() {
    let entities = relayed_with(&json!({"join": "inline"}), vec![held_city()]).await;

    pretty_assertions::assert_eq!(ids(&entities), vec![VEHICLE]);
    pretty_assertions::assert_eq!(entities[0]["locatedIn"]["entity"]["id"], json!(CITY), "{}", entities[0]);
    pretty_assertions::assert_eq!(entities[0]["locatedIn"]["entity"]["name"]["value"], json!("Paris"), "{}", entities[0]);
}

/// Clause 4.5.23.3 with clause 5.8.6: a relayed copy of a `join=flat`
/// subscription carries the Linked Entity after the relayed entity.
#[tokio::test]
async fn a_flat_join_appends_the_held_linked_entity_after_the_relayed_entity() {
    let entities = relayed_with(&json!({"join": "flat"}), vec![held_city()]).await;

    pretty_assertions::assert_eq!(ids(&entities), vec![VEHICLE, CITY]);
    pretty_assertions::assert_eq!(entities[0]["locatedIn"].get("entity"), None, "{}", entities[0]);
}

/// Clause 5.8.6: a subscription naming no `join` sends the relayed entity
/// alone, whatever it links to.
#[tokio::test]
async fn without_a_join_the_relayed_entity_is_sent_alone() {
    let entities = relayed_with(&json!({}), vec![held_city()]).await;

    pretty_assertions::assert_eq!(ids(&entities), vec![VEHICLE]);
    pretty_assertions::assert_eq!(entities[0]["locatedIn"].get("entity"), None, "{}", entities[0]);
}

/// Clause 4.5.23.1: `join=@none` retrieves no Linked Entity.
#[tokio::test]
async fn a_none_join_sends_the_relayed_entity_alone() {
    let entities = relayed_with(&json!({"join": "@none"}), vec![held_city()]).await;

    pretty_assertions::assert_eq!(ids(&entities), vec![VEHICLE]);
    pretty_assertions::assert_eq!(entities[0]["locatedIn"].get("entity"), None, "{}", entities[0]);
}

/// Clause 5.8.6: a Linked Entity this broker does not hold is left out, and
/// the notification is still sent carrying the relayed entity.
#[tokio::test]
async fn an_unheld_linked_entity_leaves_the_relayed_entity_alone() {
    for join in ["inline", "flat"] {
        let entities = relayed_with(&json!({"join": join}), Vec::new()).await;

        pretty_assertions::assert_eq!(ids(&entities), vec![VEHICLE], "join={join}");
    }
}
