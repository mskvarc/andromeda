//! What the distributed subscription side sends the companion and the copies
//! placed before the receiver changed (clause 5.8.1.4, the copy's notification
//! endpoint "set to that of the local Broker"), run against a service wired to
//! in-memory doubles.

use crate::{
    reassembly_harness::{
        ReassemblyScene,
        SceneSpec,
        companion_subscription,
        forwarding_maps::{ClaimKeeping, ClaimLease},
        identifier,
        scripted_answers::{InTurn, already_exists, late_acceptance},
        subscribing_source::SubscribingSource,
    },
    redirect_ledger::RedirectTarget,
};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use serde_json::{Value, json};
use std::time::Duration;
use wiremock::ResponseTemplate;

/// The registration of the source holding the copy.
const SOURCE: &str = "urn:ngsi-ld:ContextSourceRegistration:redirected";

/// The identifier of the copy the source took before the receiver changed.
const PLACED: &str = "urn:ngsi-ld:Subscription:placed-before-the-change";

/// The receiver below the origin the harness advertises.
const CURRENT_RECEIVER: &str = "http://broker.example:1056/ngsi-ld/andromeda/v1/notifications";

/// The forwarding timeout of a registration whose source answers late.
const FORWARDING_TIMEOUT: Duration = Duration::from_millis(200);

/// The copy the source took before the receiver changed.
fn placed() -> ForwardedCopy {
    ForwardedCopy {
        registration: identifier(SOURCE),
        remote_subscription: identifier(PLACED),
    }
}

/// The ledger's name for the copy the source took before the receiver changed.
fn placed_copy() -> RedirectTarget {
    RedirectTarget::Copy(placed())
}

/// A scene whose store holds `registration`, whose source holds the copy
/// [`PLACED`], and whose local subscription takes `members` over the defaults.
async fn scene_holding_the_copy(registration: &ContextSourceRegistration, members: Value) -> ReassemblyScene {
    ReassemblyScene::start(SceneSpec {
        subscription_members: members,
        registrations: vec![registration.clone()],
        copies: vec![placed()],
        ..SceneSpec::default()
    })
    .await
}

/// The path the source answers the copy `id` under.
fn copy_path(id: &str) -> String {
    format!("/ngsi-ld/v1/subscriptions/{id}")
}

/// The notification endpoint URI a subscription body names.
fn endpoint_of(body: &Value) -> &Value {
    &body["notification"]["endpoint"]["uri"]
}

#[tokio::test]
async fn a_copy_placed_before_the_receiver_changed_is_updated_at_its_source_to_the_current_receiver() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await.advertising_updates();
    let scene = scene_holding_the_copy(&source.stored(), json!({})).await;

    scene.adopt_receiver().await.expect("the receiver is adopted");
    assert!(scene.maps().owes(&placed_copy()), "a copy placed before the change owes the current receiver");
    scene.redirect_overdue().await.expect("the pass runs");

    let updated = source.updated().await;
    pretty_assertions::assert_eq!(updated.len(), 1);
    pretty_assertions::assert_eq!(updated[0].0, copy_path(PLACED));
    pretty_assertions::assert_eq!(endpoint_of(&updated[0].1), &json!(CURRENT_RECEIVER));
    assert!(!scene.maps().owes(&placed_copy()), "the copy the source took owes nothing");
    pretty_assertions::assert_eq!(source.placed().await, Vec::<Value>::new());
}

#[tokio::test]
async fn a_copy_that_names_the_current_receiver_is_not_sent_again() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await.advertising_updates();
    let scene = scene_holding_the_copy(&source.stored(), json!({})).await;

    scene.adopt_receiver().await.expect("the receiver is adopted");
    scene.redirect_overdue().await.expect("the pass runs");
    scene.adopt_receiver().await.expect("the receiver is adopted again");
    scene.maps().end_redirect_retries();
    scene.redirect_overdue().await.expect("the pass runs");

    pretty_assertions::assert_eq!(source.updated().await.len(), 1);
    assert!(!scene.maps().owes(&placed_copy()));
}

#[tokio::test]
async fn a_companion_placed_before_the_receiver_changed_is_updated_to_notify_the_current_receiver() {
    let scene = ReassemblyScene::start(SceneSpec::default()).await;

    scene.adopt_receiver().await.expect("the receiver is adopted");
    assert!(scene.maps().owes(&RedirectTarget::Companion));
    scene.redirect_overdue().await.expect("the pass runs");

    pretty_assertions::assert_eq!(scene.endpoints_written(&companion_subscription()), vec![CURRENT_RECEIVER.to_owned()]);
    assert!(!scene.maps().owes(&RedirectTarget::Companion));
}

/// A 503 (RFC 9110 section 15.6.4) depends on the moment the update came, so
/// the copy stays owed, is left alone by a pass before its retry falls due,
/// and is updated by the first pass after.
#[tokio::test]
async fn a_copy_whose_source_gives_no_verdict_stays_owed_and_is_updated_once_its_retry_falls_due() {
    let source = SubscribingSource::start_updating(&identifier(SOURCE), InTurn::new(ResponseTemplate::new(503), ResponseTemplate::new(204)))
        .await
        .advertising_updates();
    let scene = scene_holding_the_copy(&source.stored(), json!({})).await;

    scene.adopt_receiver().await.expect("the receiver is adopted");
    scene.redirect_overdue().await.expect("the pass runs");
    pretty_assertions::assert_eq!(source.updated().await.len(), 1);
    assert!(scene.maps().owes_later(&placed_copy()), "the copy stays owed with its retry ahead");

    scene.redirect_overdue().await.expect("the pass runs");
    pretty_assertions::assert_eq!(source.updated().await.len(), 1);

    scene.maps().end_redirect_retries();
    scene.redirect_overdue().await.expect("the pass runs");
    let updated = source.updated().await;
    pretty_assertions::assert_eq!(updated.len(), 2);
    pretty_assertions::assert_eq!(endpoint_of(&updated[1].1), &json!(CURRENT_RECEIVER));
    assert!(!scene.maps().owes(&placed_copy()));
}

/// A 404 says the source holds no copy under the identifier, which no update
/// mends, so the copy is withdrawn and placed again under a new identifier,
/// notifying the current receiver.
#[tokio::test]
async fn a_copy_its_source_no_longer_holds_is_placed_again_under_a_new_identifier() {
    let not_found = ResponseTemplate::new(404).set_body_json(json!({
        "type": "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
        "title": "Resource not found",
        "detail": "no subscription with this id"
    }));
    let source = SubscribingSource::start_updating(&identifier(SOURCE), not_found).await.advertising_updates();
    let scene = scene_holding_the_copy(&source.stored(), json!({})).await;

    scene.adopt_receiver().await.expect("the receiver is adopted");
    scene.redirect_overdue().await.expect("the pass runs");

    pretty_assertions::assert_eq!(source.updated().await.len(), 1);
    pretty_assertions::assert_eq!(source.deleted().await, vec![copy_path(PLACED)]);
    let placed_again = source.placed().await;
    pretty_assertions::assert_eq!(placed_again.len(), 1);
    assert_ne!(placed_again[0]["id"], json!(PLACED), "the copy is placed under a new identifier");
    pretty_assertions::assert_eq!(endpoint_of(&placed_again[0]), &json!(CURRENT_RECEIVER));
    pretty_assertions::assert_eq!(scene.removed_mappings(), vec![identifier(SOURCE)]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), vec![identifier(PLACED)]);
    assert!(!scene.maps().owes(&placed_copy()));
}

/// Clause 5.8.1.4 updates a copy only at a source whose registration
/// indicates support for the Update Subscription operation, so a copy at one
/// that does not is withdrawn and placed again, notifying the current
/// receiver.
#[tokio::test]
async fn a_copy_at_a_registration_that_takes_no_update_is_placed_again() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_holding_the_copy(&source.stored(), json!({})).await;

    scene.adopt_receiver().await.expect("the receiver is adopted");
    scene.redirect_overdue().await.expect("the pass runs");

    pretty_assertions::assert_eq!(source.updated().await, Vec::<(String, Value)>::new());
    pretty_assertions::assert_eq!(source.deleted().await, vec![copy_path(PLACED)]);
    let placed_again = source.placed().await;
    pretty_assertions::assert_eq!(placed_again.len(), 1);
    pretty_assertions::assert_eq!(endpoint_of(&placed_again[0]), &json!(CURRENT_RECEIVER));
    assert!(!scene.maps().owes(&placed_copy()));
}

/// A send that got no answer in time may have left the copy at the source,
/// and the source answers the copy sent again with `AlreadyExists`. What it
/// holds is what the earlier send left, which may name a receiver this broker
/// no longer serves, so the settled copy owes the current receiver and the
/// pass updates it.
#[tokio::test]
async fn a_copy_its_source_already_held_when_its_claim_settled_is_updated_to_the_current_receiver() {
    let source = SubscribingSource::start_with(
        &identifier(SOURCE),
        InTurn::new(late_acceptance(), already_exists()),
        ResponseTemplate::new(204),
    )
    .await
    .advertising_updates();
    let registration = source.stored_timing_out_after(FORWARDING_TIMEOUT);
    let scene = ReassemblyScene::start(SceneSpec {
        registrations: vec![registration.clone()],
        claims: ClaimKeeping::Kept,
        ..SceneSpec::default()
    })
    .await;

    scene.place_as_at_creation(&registration).await.expect("the placement is settled");
    let claimed = scene.maps().claimed()[0].remote_subscription.clone();
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), Some(ClaimLease::Settled));
    let held = RedirectTarget::Copy(ForwardedCopy {
        registration: identifier(SOURCE),
        remote_subscription: claimed.clone(),
    });
    assert!(scene.maps().owes(&held), "a copy an earlier send left owes the current receiver");

    scene.redirect_overdue().await.expect("the pass runs");

    let updated = source.updated().await;
    pretty_assertions::assert_eq!(updated.len(), 1);
    pretty_assertions::assert_eq!(updated[0].0, copy_path(claimed.as_str()));
    pretty_assertions::assert_eq!(endpoint_of(&updated[0].1), &json!(CURRENT_RECEIVER));
    assert!(!scene.maps().owes(&held));
}

/// Clause 5.8.2.4 updates the companion and the copies with every update of
/// the subscription, and that update names the current receiver, so nothing
/// is owed after it and the pass sends nothing more.
#[tokio::test]
async fn an_update_of_the_subscription_settles_what_the_companion_and_the_copy_owed() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await.advertising_updates();
    let scene = scene_holding_the_copy(&source.stored(), json!({})).await;

    scene.adopt_receiver().await.expect("the receiver is adopted");
    scene.update_to_stored().await.expect("the update is followed");

    assert!(!scene.maps().owes(&RedirectTarget::Companion));
    assert!(!scene.maps().owes(&placed_copy()));
    scene.maps().end_redirect_retries();
    scene.redirect_overdue().await.expect("the pass runs");
    pretty_assertions::assert_eq!(source.updated().await.len(), 1);
}

/// An expired subscription takes part in no distributed operation (clause
/// 5.8.1.4) and its expiry withdraws its copies, so its copy is sent nothing
/// and stays owed until then.
#[tokio::test]
async fn a_copy_of_an_expired_subscription_is_sent_nothing_and_its_update_is_postponed() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await.advertising_updates();
    let scene = scene_holding_the_copy(&source.stored(), json!({"status": "expired"})).await;

    scene.adopt_receiver().await.expect("the receiver is adopted");
    scene.redirect_overdue().await.expect("the pass runs");

    pretty_assertions::assert_eq!(source.updated().await, Vec::<(String, Value)>::new());
    assert!(scene.maps().owes_later(&placed_copy()), "the update waits for the expiry's withdrawal");
}
