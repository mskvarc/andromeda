//! What the distributed subscription side places at a context source whose
//! registration starts to match after the local subscription was created
//! (clauses 5.8.1.4 and 5.11.7), run against a service wired to in-memory
//! doubles.

use crate::reassembly_harness::{
    ReassemblyScene,
    SceneSpec,
    companion_subscription,
    default_iri,
    identifier,
    subscribing_source::{SPEED, SubscribingSource},
};
use serde_json::{Value, json};

/// The `jsonldContext` a subscription created without a `@context` is stored
/// with (clause 5.8.1.4): the v1.9 core context.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The registration of the source the copy is placed at.
const SOURCE: &str = "urn:ngsi-ld:ContextSourceRegistration:late";

/// A `newlyMatching` notification of the companion naming `registration`.
fn newly_matching(registration: &Value) -> Value {
    json!({
        "id": "urn:ngsi-ld:Notification:companion",
        "type": "ContextSourceNotification",
        "subscriptionId": companion_subscription().as_str(),
        "notifiedAt": "2026-10-28T12:00:00Z",
        "triggerReason": "newlyMatching",
        "data": [registration]
    })
}

/// A scene whose local subscription watches [`SPEED`] of `Vehicle` under the
/// core context, and whose registration store holds `source`.
async fn scene_with(source: &SubscribingSource) -> ReassemblyScene {
    ReassemblyScene::start(SceneSpec {
        subscription_members: json!({
            "watchedAttributes": [SPEED],
            "jsonldContext": CORE_CONTEXT
        }),
        registrations: vec![source.stored()],
        ..SceneSpec::default()
    })
    .await
}

/// `placed` without the identifier minted for it, which differs from one
/// placement to the next.
fn without_id(mut placed: Value) -> Value {
    placed.as_object_mut().expect("a placed subscription is an object").remove("id");
    placed
}

/// Clause 5.8.1.4: the companion notification writes the registration compacted
/// under the companion's context, and the copy is still reduced against the
/// registration the store holds, so it keeps the entity selector and the
/// watched attribute the source registers.
#[tokio::test]
async fn a_registration_matching_after_creation_receives_the_copy_a_registration_matching_at_creation_receives() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_with(&source).await;

    scene.place_as_at_creation(&source.stored()).await.expect("the copy is placed at creation");
    scene.post(&newly_matching(&source.notified())).await.expect("the notification is handled");

    let placed = source.placed().await;
    let [at_creation, late] = placed.as_slice() else {
        panic!("two copies are placed, one per placement: {placed:?}");
    };
    pretty_assertions::assert_eq!(late["entities"], json!([{"type": default_iri("Vehicle")}]));
    pretty_assertions::assert_eq!(late["watchedAttributes"], json!([SPEED]));
    pretty_assertions::assert_eq!(without_id(late.clone()), without_id(at_creation.clone()));
}

/// Clause 5.8.1.4: an expired subscription sends no notification, so a
/// companion notification handled while the expiry withdraws its copies places
/// none at a source it already relieved.
#[tokio::test]
async fn a_companion_notification_for_an_expired_subscription_places_nothing() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"watchedAttributes": [SPEED], "status": "expired"}),
        registrations: vec![source.stored()],
        ..SceneSpec::default()
    })
    .await;

    scene.post(&newly_matching(&source.notified())).await.expect("the notification is handled");

    pretty_assertions::assert_eq!(source.placed().await, Vec::<Value>::new());
}

/// A registration deleted before its companion notification is handled has had
/// its copies torn down with it, so nothing is placed for it.
#[tokio::test]
async fn a_companion_notification_naming_a_registration_no_longer_stored_places_nothing() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"watchedAttributes": [SPEED]}),
        ..SceneSpec::default()
    })
    .await;

    scene.post(&newly_matching(&source.notified())).await.expect("the notification is handled");

    pretty_assertions::assert_eq!(source.placed().await, Vec::<Value>::new());
}
