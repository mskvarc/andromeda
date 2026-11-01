//! The lifecycle steps of one local subscription run one after the other
//! (clauses 5.8.1.4 and 5.8.2.4), against a service wired to in-memory
//! doubles.

use crate::reassembly_harness::{
    ReassemblyScene,
    SceneSpec,
    StoredCompanion,
    companion_subscription,
    copy_subscription,
    identifier,
    subscribing_source::{SPEED, SubscribingSource},
};
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use serde_json::{Value, json};
use tokio::task::yield_now;

/// The registration of the source holding the copy.
const SOURCE: &str = "urn:ngsi-ld:ContextSourceRegistration:holder";

/// The members of the local subscription once an update wrote an `expiresAt`
/// in the future.
fn revived() -> Value {
    json!({"watchedAttributes": [SPEED], "expiresAt": "2999-01-01T00:00:00Z"})
}

/// A scene whose local subscription takes `members` over, whose registration
/// store holds `source`, and whose mappings name `copies` and a companion the
/// store holds as `companion` says.
async fn scene_at(source: &SubscribingSource, members: Value, copies: Vec<ForwardedCopy>, companion: StoredCompanion) -> ReassemblyScene {
    ReassemblyScene::start(SceneSpec {
        subscription_members: members,
        registrations: vec![source.stored()],
        copies,
        companion,
        ..SceneSpec::default()
    })
    .await
}

/// The copy `source` holds.
fn held_copy() -> Vec<ForwardedCopy> {
    vec![ForwardedCopy {
        registration: identifier(SOURCE),
        remote_subscription: copy_subscription(),
    }]
}

/// The path a deletion of the copy is sent to.
fn copy_path() -> String {
    format!("/ngsi-ld/v1/subscriptions/{}", copy_subscription().as_str())
}

/// Clause 5.8.2.4: a future `expiresAt` makes an expired subscription active
/// again. The update starts after the expiry deleted the copy and before it
/// deleted the companion, so it waits for the withdrawal to end and then
/// provisions a companion that places the copy again.
#[tokio::test]
async fn an_update_reviving_a_subscription_mid_withdrawal_places_its_copy_again() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_at(
        &source,
        json!({"watchedAttributes": [SPEED], "status": "expired"}),
        held_copy(),
        StoredCompanion::Held,
    )
    .await;
    scene.hold_deletions();

    let (expired, updated) = tokio::join!(scene.expire(), async {
        scene.deletion_reached().await;
        pretty_assertions::assert_eq!(source.deleted().await, vec![copy_path()]);
        scene.store_local(&revived());
        // The update is polled first, so it has started by the time the
        // companion's deletion is let through.
        let (updated, ()) = tokio::join!(scene.update_to_stored(), async {
            yield_now().await;
            scene.release_deletions();
        });
        updated
    });

    expired.expect("the expiry is followed");
    updated.expect("the update is followed");
    let companion = scene.linked_companion();
    assert_ne!(companion, companion_subscription(), "a new companion is linked");
    assert!(scene.stores(&companion), "the new companion is stored");
    assert!(!scene.stores(&companion_subscription()), "the expired companion is deleted");
    pretty_assertions::assert_eq!(source.deleted().await, vec![copy_path()]);
    pretty_assertions::assert_eq!(source.placed().await.len(), 1);
}

/// Clause 5.8.2.4: an update that made the expired subscription active again
/// and was followed before the expiry step ran leaves that step nothing to
/// withdraw.
#[tokio::test]
async fn an_expiry_step_after_a_reviving_update_withdraws_nothing() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_at(&source, revived(), held_copy(), StoredCompanion::Held).await;

    scene.update_to_stored().await.expect("the update is followed");
    scene.expire().await.expect("the expiry is followed");

    pretty_assertions::assert_eq!(source.deleted().await, Vec::<String>::new());
    pretty_assertions::assert_eq!(scene.removed_mappings(), Vec::new());
    pretty_assertions::assert_eq!(scene.deleted_subscriptions(), Vec::new());
    pretty_assertions::assert_eq!(scene.linked_companion(), companion_subscription());
    assert!(scene.stores(&companion_subscription()), "the companion stays");
}

/// Clause 5.8.2.4: an update whose linked companion the store no longer holds
/// provisions a new companion, which places the copy the registration calls
/// for.
#[tokio::test]
async fn an_update_whose_companion_is_missing_provisions_a_new_one_and_places_the_copy() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_at(&source, json!({"watchedAttributes": [SPEED]}), Vec::new(), StoredCompanion::Missing).await;

    scene.update_to_stored().await.expect("the update is followed");

    let companion = scene.linked_companion();
    assert_ne!(companion, companion_subscription(), "a new companion is linked");
    assert!(scene.stores(&companion), "the new companion is stored");
    pretty_assertions::assert_eq!(source.placed().await.len(), 1);
}

/// Two update steps of one subscription started together run one after the
/// other: the companion is never updated by both at once.
#[tokio::test]
async fn two_update_steps_of_one_subscription_run_one_after_the_other() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_at(&source, json!({"watchedAttributes": [SPEED]}), held_copy(), StoredCompanion::Held).await;

    let (first, second) = tokio::join!(scene.update_to_stored(), scene.update_to_stored());

    first.expect("the first update is followed");
    second.expect("the second update is followed");
    pretty_assertions::assert_eq!(scene.most_updates_at_once(), 1);
}
