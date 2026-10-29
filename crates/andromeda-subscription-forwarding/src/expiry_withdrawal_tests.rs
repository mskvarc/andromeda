//! What the expiry of a local subscription leaves at the context sources
//! holding its copies (clause 5.8.1.4), run against a service wired to
//! in-memory doubles: the same as its deletion would (clause 5.8.5.4).

use crate::reassembly_harness::{
    ReassemblyScene,
    SceneSpec,
    companion_subscription,
    copy_subscription,
    identifier,
    subscribing_source::{SPEED, SubscribingSource},
};
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use serde_json::{Value, json};

/// The registration of the source holding the copy.
const SOURCE: &str = "urn:ngsi-ld:ContextSourceRegistration:holder";

/// A scene whose local subscription takes `members` over and whose copy is
/// held at `source`.
async fn scene_holding_a_copy_at(source: &SubscribingSource, members: Value) -> ReassemblyScene {
    ReassemblyScene::start(SceneSpec {
        subscription_members: members,
        registrations: vec![source.stored()],
        copies: vec![ForwardedCopy {
            registration: identifier(SOURCE),
            remote_subscription: copy_subscription(),
        }],
        ..SceneSpec::default()
    })
    .await
}

/// The path a deletion of the copy is sent to.
fn copy_path() -> String {
    format!("/ngsi-ld/v1/subscriptions/{}", copy_subscription().as_str())
}

/// Clause 5.8.1.4: an expired subscription sends no notification, so its copy
/// is deleted at the source, its mapping removed with the local row still in
/// place, and its companion deleted.
#[tokio::test]
async fn an_expiry_deletes_the_copies_at_the_sources_and_the_companion() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_holding_a_copy_at(&source, json!({"watchedAttributes": [SPEED], "status": "expired"})).await;

    scene.expire().await.expect("the expiry is followed");

    pretty_assertions::assert_eq!(source.deleted().await, vec![copy_path()]);
    pretty_assertions::assert_eq!(scene.removed_mappings(), vec![identifier(SOURCE)]);
    pretty_assertions::assert_eq!(scene.deleted_subscriptions(), vec![companion_subscription()]);
}

/// Clause 5.8.2.4: an update keeping the subscription expired, its
/// `expiresAt` passed before the sweep reached it, withdraws it as the
/// expiry does instead of following the update at the source.
#[tokio::test]
async fn an_update_leaving_the_subscription_expired_withdraws_it() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_holding_a_copy_at(&source, json!({"watchedAttributes": [SPEED], "expiresAt": "2020-06-01T00:00:00Z"})).await;

    scene.update_to_stored().await.expect("the update is followed");

    pretty_assertions::assert_eq!(source.deleted().await, vec![copy_path()]);
    pretty_assertions::assert_eq!(source.placed().await, Vec::<Value>::new());
    pretty_assertions::assert_eq!(scene.removed_mappings(), vec![identifier(SOURCE)]);
    pretty_assertions::assert_eq!(scene.deleted_subscriptions(), vec![companion_subscription()]);
}
