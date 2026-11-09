//! What becomes of a reduced copy whose placement was cut off before its
//! source answered (clause 5.8.1.4), run against a service wired to in-memory
//! doubles whose mappings keep every claim as the store keeps it.

use crate::reassembly_harness::{
    ReassemblyScene,
    SceneSpec,
    forwarding_maps::{ClaimKeeping, ClaimLease},
    identifier,
    subscribing_source::SubscribingSource,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex, time::Duration};
use tokio::time::timeout;
use wiremock::{Request, Respond, ResponseTemplate};

/// The registration of the source the copy is placed at.
const SOURCE: &str = "urn:ngsi-ld:ContextSourceRegistration:resumed";

/// How long a test lets a placement held on its routing run before it drops
/// it, as the request deadline would.
const CUT_OFF_AFTER: Duration = Duration::from_millis(200);

/// The forwarding timeout of a registration whose source answers late.
const FORWARDING_TIMEOUT: Duration = Duration::from_millis(200);

/// Answers each placement with the next answer it holds, and every placement
/// past them with its last one.
struct InTurn {
    /// The answers not given yet, in order.
    answers: Mutex<VecDeque<ResponseTemplate>>,
    /// The answer once the others are given.
    last: ResponseTemplate,
}

impl InTurn {
    /// `first` answered once, then `last` to every placement after it.
    fn new(first: ResponseTemplate, last: ResponseTemplate) -> InTurn {
        InTurn {
            answers: Mutex::new(VecDeque::from([first])),
            last,
        }
    }
}

impl Respond for InTurn {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.answers
            .lock()
            .expect("the answers are never poisoned")
            .pop_front()
            .unwrap_or_else(|| self.last.clone())
    }
}

/// An acceptance that comes after [`FORWARDING_TIMEOUT`], so the placement
/// gets no answer in time while the source may hold the copy.
fn late_acceptance() -> ResponseTemplate {
    ResponseTemplate::new(201).set_delay(Duration::from_secs(2))
}

/// The answer of a source asked to create a subscription it already holds
/// (`AlreadyExists`, clause 5.5.2).
fn already_exists() -> ResponseTemplate {
    ResponseTemplate::new(409).set_body_json(json!({
        "type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists",
        "title": "Already exists",
        "detail": "a subscription with this id already exists"
    }))
}

/// A scene whose mappings keep every claim, and whose registration store holds
/// `registrations`.
async fn scene_keeping(registrations: Vec<ContextSourceRegistration>) -> ReassemblyScene {
    ReassemblyScene::start(SceneSpec {
        registrations,
        claims: ClaimKeeping::Kept,
        ..SceneSpec::default()
    })
    .await
}

/// Place a copy at `registration` with its routing held, and drop the
/// placement once it has waited [`CUT_OFF_AFTER`], after its claim and before
/// its send.
async fn cut_off_placement(scene: &ReassemblyScene, registration: &ContextSourceRegistration) {
    scene.router().hold_routing();
    let cut_off = timeout(CUT_OFF_AFTER, scene.place_as_at_creation(registration)).await;
    assert!(cut_off.is_err(), "the placement waits on its held routing until it is dropped");
    scene.router().release_routing();
}

/// The identifier of every copy the source received, in the order they came.
async fn placed_ids(source: &SubscribingSource) -> Vec<Identifier> {
    source
        .placed()
        .await
        .iter()
        .map(|copy| identifier(copy["id"].as_str().expect("a placed copy names its identifier")))
        .collect()
}

/// The identifier the first claim at [`SOURCE`] recorded.
fn first_claimed(scene: &ReassemblyScene) -> Identifier {
    let claimed = scene.maps().claimed();
    let first = claimed.first().expect("a copy was claimed");
    pretty_assertions::assert_eq!(first.registration, identifier(SOURCE));
    first.remote_subscription.clone()
}

/// Whether the copy at [`SOURCE`] is kept under a lease still running or
/// ended, rather than settled or gone.
fn leased(scene: &ReassemblyScene) -> bool {
    matches!(scene.maps().lease_at(&identifier(SOURCE)), Some(ClaimLease::Until(_)))
}

/// A notification the source sends under the copy `copy`, of one `Vehicle`.
fn notification_under(copy: &Identifier) -> Value {
    json!({
        "id": "urn:ngsi-ld:Notification:resumed",
        "type": "Notification",
        "subscriptionId": copy.as_str(),
        "notifiedAt": "2026-11-09T12:00:00Z",
        "data": [{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 40}}]
    })
}

/// A placement answered at once is settled, so the resumption has nothing to
/// place again even after every lease has run out.
#[tokio::test]
async fn a_copy_its_source_accepted_is_settled_and_never_placed_again() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_keeping(vec![source.stored()]).await;

    scene.place_as_at_creation(&source.stored()).await.expect("the copy is placed");
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, vec![first_claimed(&scene)]);
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), Some(ClaimLease::Settled));
    pretty_assertions::assert_eq!(scene.maps().settled(), vec![identifier(SOURCE)]);
}

/// A placement the request deadline cuts off between its claim and its send
/// leaves its claim and no copy at the source. While the claim's lease runs, a
/// second placement leaves it to the first and sends nothing. Once the lease
/// has run out, the resumption sends the copy under the identifier claimed,
/// the claim is settled, and a notification the source sends under the copy
/// reaches the subscriber.
#[tokio::test]
async fn a_placement_cut_off_between_its_claim_and_its_send_is_placed_again_once_its_lease_runs_out() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let mut scene = scene_keeping(vec![source.stored()]).await;

    cut_off_placement(&scene, &source.stored()).await;
    let claimed = first_claimed(&scene);
    pretty_assertions::assert_eq!(placed_ids(&source).await, Vec::<Identifier>::new());
    assert!(leased(&scene), "the cut-off claim stays under its lease");

    scene.place_as_at_creation(&source.stored()).await.expect("the second placement is settled");
    scene.resume_stale_placements().await.expect("the pass runs");
    pretty_assertions::assert_eq!(placed_ids(&source).await, Vec::<Identifier>::new());
    pretty_assertions::assert_eq!(scene.maps().claimed().len(), 1);

    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, vec![claimed.clone()]);
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), Some(ClaimLease::Settled));
    pretty_assertions::assert_eq!(scene.router().registered(), vec![claimed.clone()]);
    scene.post(&notification_under(&claimed)).await.expect("the notification is handled");
    let delivered = scene.delivered().await.expect("the subscriber is sent the notification");
    pretty_assertions::assert_eq!(delivered["data"][0]["id"], json!("urn:ngsi-ld:Vehicle:1"));
}

/// Past the lease, a later placement takes the abandoned claim over and sends
/// the copy under the identifier claimed, not under one minted for it.
#[tokio::test]
async fn a_later_placement_takes_an_abandoned_claim_over_under_its_identifier() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_keeping(vec![source.stored()]).await;

    cut_off_placement(&scene, &source.stored()).await;
    let claimed = first_claimed(&scene);
    scene.maps().end_claim_leases();
    scene.place_as_at_creation(&source.stored()).await.expect("the copy is placed");

    pretty_assertions::assert_eq!(placed_ids(&source).await, vec![claimed.clone()]);
    let taken_over: Vec<Identifier> = scene.maps().claimed().into_iter().map(|claim| claim.remote_subscription).collect();
    pretty_assertions::assert_eq!(taken_over, vec![claimed.clone(), claimed]);
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), Some(ClaimLease::Settled));
}

/// A source that held the copy without answering in time answers the copy
/// sent again with `AlreadyExists`, which settles the claim and keeps the
/// route, so the source holds one copy.
#[tokio::test]
async fn a_copy_that_got_no_answer_in_time_is_sent_again_and_settled_by_already_exists() {
    let source = SubscribingSource::start_answering(&identifier(SOURCE), InTurn::new(late_acceptance(), already_exists())).await;
    let registration = source.stored_timing_out_after(FORWARDING_TIMEOUT);
    let scene = scene_keeping(vec![registration.clone()]).await;

    scene.place_as_at_creation(&registration).await.expect("the placement is settled");
    let claimed = first_claimed(&scene);
    assert!(leased(&scene), "a copy that got no answer stays claimed");
    pretty_assertions::assert_eq!(scene.router().registered(), vec![claimed.clone()]);

    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, vec![claimed.clone(), claimed.clone()]);
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), Some(ClaimLease::Settled));
    pretty_assertions::assert_eq!(scene.router().registered(), vec![claimed.clone(), claimed]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), Vec::<Identifier>::new());
    pretty_assertions::assert_eq!(scene.removed_mappings(), Vec::<Identifier>::new());
}

/// A source that never stored the copy it did not answer in time accepts the
/// copy sent again, which settles the claim.
#[tokio::test]
async fn a_copy_that_got_no_answer_in_time_is_sent_again_and_settled_by_its_acceptance() {
    let source = SubscribingSource::start_answering(&identifier(SOURCE), InTurn::new(late_acceptance(), ResponseTemplate::new(201))).await;
    let registration = source.stored_timing_out_after(FORWARDING_TIMEOUT);
    let scene = scene_keeping(vec![registration.clone()]).await;

    scene.place_as_at_creation(&registration).await.expect("the placement is settled");
    let claimed = first_claimed(&scene);
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, vec![claimed.clone(), claimed]);
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), Some(ClaimLease::Settled));
    pretty_assertions::assert_eq!(scene.router().unregistered(), Vec::<Identifier>::new());
}

/// A copy sent again and refused leaves nothing at the source, so the claim
/// and the route are released.
#[tokio::test]
async fn a_copy_sent_again_and_refused_is_released() {
    let source = SubscribingSource::start_answering(&identifier(SOURCE), InTurn::new(late_acceptance(), ResponseTemplate::new(400))).await;
    let registration = source.stored_timing_out_after(FORWARDING_TIMEOUT);
    let scene = scene_keeping(vec![registration.clone()]).await;

    scene.place_as_at_creation(&registration).await.expect("the placement is settled");
    let claimed = first_claimed(&scene);
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, vec![claimed.clone(), claimed.clone()]);
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), None);
    pretty_assertions::assert_eq!(scene.removed_mappings(), vec![identifier(SOURCE)]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), vec![claimed]);
}

/// An expired subscription takes part in no distributed operation (clause
/// 5.8.1.4) and its expiry withdraws its copies, so its abandoned claim is
/// not placed again.
#[tokio::test]
async fn an_abandoned_claim_of_an_expired_subscription_is_not_placed_again() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_keeping(vec![source.stored()]).await;

    cut_off_placement(&scene, &source.stored()).await;
    scene.store_local(&json!({"status": "expired"}));
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, Vec::<Identifier>::new());
    assert!(leased(&scene), "the claim is left to the expiry");
}

/// An auxiliary source never takes a copy (clause 5.8.1.4), so an abandoned
/// claim at a registration made auxiliary since is withdrawn from the source,
/// which may hold the copy, and released rather than left overdue.
#[tokio::test]
async fn an_abandoned_claim_at_a_registration_made_auxiliary_is_withdrawn() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_keeping(vec![source.stored_as_auxiliary()]).await;

    cut_off_placement(&scene, &source.stored()).await;
    let claimed = first_claimed(&scene);
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, Vec::<Identifier>::new());
    pretty_assertions::assert_eq!(source.deleted().await, vec![format!("/ngsi-ld/v1/subscriptions/{claimed}")]);
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), None);
    pretty_assertions::assert_eq!(scene.removed_mappings(), vec![identifier(SOURCE)]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), vec![claimed]);
}

/// A registration that no longer advertises the creation of subscriptions
/// takes no copy (clause 5.8.1.4), so an abandoned claim at it is withdrawn
/// from the source, which may hold the copy, and released rather than left
/// overdue.
#[tokio::test]
async fn an_abandoned_claim_at_a_registration_that_stopped_taking_copies_is_withdrawn() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_keeping(vec![source.stored_without_subscription_creation()]).await;

    cut_off_placement(&scene, &source.stored()).await;
    let claimed = first_claimed(&scene);
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, Vec::<Identifier>::new());
    pretty_assertions::assert_eq!(source.deleted().await, vec![format!("/ngsi-ld/v1/subscriptions/{claimed}")]);
    pretty_assertions::assert_eq!(scene.maps().lease_at(&identifier(SOURCE)), None);
    pretty_assertions::assert_eq!(scene.removed_mappings(), vec![identifier(SOURCE)]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), vec![claimed]);
}

/// The copy of a claim taken over may have reached its source before, so a
/// send again that never reaches the source (here a refused connection)
/// learns nothing of it. The claim and the route stay under a new lease, which
/// a pass right after leaves alone, and nothing is removed, so a pass once the
/// lease has run out sends the copy once more.
#[tokio::test]
async fn a_copy_sent_again_that_never_reached_its_source_stays_claimed() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_keeping(vec![source.stored_at_a_closed_port()]).await;

    cut_off_placement(&scene, &source.stored()).await;
    let claimed = first_claimed(&scene);
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");
    scene.resume_stale_placements().await.expect("the pass runs");

    let taken_over: Vec<Identifier> = scene.maps().claimed().into_iter().map(|claim| claim.remote_subscription).collect();
    pretty_assertions::assert_eq!(taken_over, vec![claimed.clone(), claimed.clone()]);
    assert!(leased(&scene), "the claim taken over stays under a lease");
    pretty_assertions::assert_eq!(scene.router().registered(), vec![claimed]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), Vec::<Identifier>::new());
    pretty_assertions::assert_eq!(scene.removed_mappings(), Vec::<Identifier>::new());
}

/// A registration the store no longer holds has had its copies torn down with
/// it, so an abandoned claim naming it is not placed again.
#[tokio::test]
async fn an_abandoned_claim_of_a_registration_no_longer_stored_is_not_placed_again() {
    let source = SubscribingSource::start(&identifier(SOURCE)).await;
    let scene = scene_keeping(Vec::new()).await;

    cut_off_placement(&scene, &source.stored()).await;
    scene.maps().end_claim_leases();
    scene.resume_stale_placements().await.expect("the pass runs");

    pretty_assertions::assert_eq!(placed_ids(&source).await, Vec::<Identifier>::new());
}
