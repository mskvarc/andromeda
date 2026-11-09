//! How the distributed subscription side places one reduced copy at a context
//! source (clause 5.8.1.4): what it records and routes before the source
//! receives the copy, and what it keeps once the source has answered, run
//! against a service wired to in-memory doubles.

use crate::reassembly_harness::{
    ReassemblyScene,
    SceneSpec,
    copy_subscription,
    forwarding_maps::ForwardingMaps,
    identifier,
    recording_router::RecordingRouter,
    subscribing_source::{SPEED, SubscribingSource},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use wiremock::{Request, Respond, ResponseTemplate};

/// The registration of the source the copy is placed at.
const SOURCE: &str = "urn:ngsi-ld:ContextSourceRegistration:placed";

/// What the mappings and the route held when the source received a copy.
#[derive(Debug, Clone)]
struct AtArrival {
    /// The copies claimed by then.
    claimed: Vec<ForwardedCopy>,
    /// The identifiers routed by then.
    routed: Vec<Identifier>,
    /// The identifier the copy the source received carries.
    placed: Identifier,
}

/// A placement answer that notes what the mappings and the route held when the
/// copy arrived, and accepts it.
struct ArrivalProbe {
    /// The mappings the service records the copy in.
    maps: Arc<ForwardingMaps>,
    /// The route the service registers the copy's identifier with.
    router: Arc<RecordingRouter>,
    /// What was held when the copy arrived, once it has.
    seen: Arc<Mutex<Option<AtArrival>>>,
}

impl Respond for ArrivalProbe {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let copy: Value = serde_json::from_slice(&request.body).expect("a placed subscription is JSON");
        let placed = identifier(copy["id"].as_str().expect("the copy names its identifier"));
        *self.seen.lock().expect("the probe is never poisoned") = Some(AtArrival {
            claimed: self.maps.claimed(),
            routed: self.router.registered(),
            placed,
        });
        ResponseTemplate::new(201)
    }
}

/// A scene whose local subscription watches [`SPEED`], with the copies
/// `copies` already held at the sources.
async fn scene_holding(copies: Vec<ForwardedCopy>) -> ReassemblyScene {
    ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"watchedAttributes": [SPEED]}),
        copies,
        ..SceneSpec::default()
    })
    .await
}

/// The one copy claimed at [`SOURCE`], read off the mappings.
fn the_claim(maps: &ForwardingMaps) -> ForwardedCopy {
    let claimed = maps.claimed();
    let [claim] = claimed.as_slice() else {
        panic!("one copy is claimed: {claimed:?}");
    };
    pretty_assertions::assert_eq!(claim.registration, identifier(SOURCE));
    claim.clone()
}

/// A source answers a notification under an identifier this broker does not
/// know with nothing it retries, so the copy has to be recorded and routed by
/// the time the source holds it.
#[tokio::test]
async fn a_copy_is_recorded_and_routed_before_its_source_receives_it() {
    let scene = scene_holding(Vec::new()).await;
    let seen = Arc::new(Mutex::new(None));
    let probe = ArrivalProbe {
        maps: scene.maps(),
        router: scene.router(),
        seen: Arc::clone(&seen),
    };
    let source = SubscribingSource::start_answering(&identifier(SOURCE), probe).await;

    scene.place_as_at_creation(&source.stored()).await.expect("the copy is placed");

    let at_arrival = seen.lock().expect("the probe is never poisoned").clone().expect("the source received the copy");
    pretty_assertions::assert_eq!(
        at_arrival.claimed,
        vec![ForwardedCopy {
            registration: identifier(SOURCE),
            remote_subscription: at_arrival.placed.clone(),
        }]
    );
    pretty_assertions::assert_eq!(at_arrival.routed, vec![at_arrival.placed]);
    pretty_assertions::assert_eq!(scene.removed_mappings(), Vec::<Identifier>::new());
    pretty_assertions::assert_eq!(scene.router().unregistered(), Vec::<Identifier>::new());
}

/// A source that answers an error status holds no copy, so the claim and the
/// route are released and a later placement can claim the source again.
#[tokio::test]
async fn a_copy_the_source_refuses_is_released() {
    let scene = scene_holding(Vec::new()).await;
    let source = SubscribingSource::start_answering(&identifier(SOURCE), ResponseTemplate::new(400)).await;

    scene.place_as_at_creation(&source.stored()).await.expect("the placement is settled");

    let claim = the_claim(&scene.maps());
    pretty_assertions::assert_eq!(scene.removed_mappings(), vec![identifier(SOURCE)]);
    pretty_assertions::assert_eq!(scene.router().registered(), vec![claim.remote_subscription.clone()]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), vec![claim.remote_subscription]);
}

/// A source that does not answer within the registration's timeout may still
/// create the copy and notify under it, so the claim and the route stay.
#[tokio::test]
async fn a_copy_the_source_does_not_answer_in_time_stays_recorded_and_routed() {
    let scene = scene_holding(Vec::new()).await;
    let late = ResponseTemplate::new(201).set_delay(Duration::from_secs(5));
    let source = SubscribingSource::start_answering(&identifier(SOURCE), late).await;

    scene
        .place_as_at_creation(&source.stored_timing_out_after(Duration::from_millis(200)))
        .await
        .expect("the placement is settled");

    let claim = the_claim(&scene.maps());
    pretty_assertions::assert_eq!(scene.removed_mappings(), Vec::<Identifier>::new());
    pretty_assertions::assert_eq!(scene.router().registered(), vec![claim.remote_subscription]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), Vec::<Identifier>::new());
}

/// Clause 5.2.12: a source that echoes the copy under an identifier of its own
/// notifies under that one, so the copy is recorded and routed under it, and
/// the minted identifier is forgotten.
#[tokio::test]
async fn a_copy_the_source_holds_under_another_identifier_is_recorded_and_routed_under_it() {
    let scene = scene_holding(Vec::new()).await;
    let assigned = identifier("urn:ngsi-ld:Subscription:assigned");
    let echoed = ResponseTemplate::new(201).set_body_json(json!({"id": assigned.as_str(), "type": "Subscription"}));
    let source = SubscribingSource::start_answering(&identifier(SOURCE), echoed).await;

    scene.place_as_at_creation(&source.stored()).await.expect("the copy is placed");

    let minted = the_claim(&scene.maps()).remote_subscription;
    pretty_assertions::assert_eq!(
        scene.maps().recorded(),
        vec![ForwardedCopy {
            registration: identifier(SOURCE),
            remote_subscription: assigned.clone(),
        }]
    );
    pretty_assertions::assert_eq!(scene.router().registered(), vec![minted.clone(), assigned]);
    pretty_assertions::assert_eq!(scene.router().unregistered(), vec![minted]);
}

/// Two placements of the same copy, such as the one a creation makes and the
/// one the companion's notification asks for, send it once: the second finds
/// the first recorded and sends nothing.
#[tokio::test]
async fn a_source_already_holding_a_copy_is_sent_none() {
    let scene = scene_holding(vec![ForwardedCopy {
        registration: identifier(SOURCE),
        remote_subscription: copy_subscription(),
    }])
    .await;
    let source = SubscribingSource::start(&identifier(SOURCE)).await;

    scene.place_as_at_creation(&source.stored()).await.expect("the placement is settled");

    pretty_assertions::assert_eq!(source.placed().await, Vec::<Value>::new());
    pretty_assertions::assert_eq!(scene.maps().claimed(), Vec::<ForwardedCopy>::new());
    pretty_assertions::assert_eq!(scene.router().registered(), Vec::<Identifier>::new());
}

/// Clause 5.8.1.4 forwards a subscription only to a registration that
/// advertises its creation, so one that does not is neither claimed nor sent
/// anything.
#[tokio::test]
async fn a_registration_not_advertising_subscription_creation_is_neither_claimed_nor_sent_a_copy() {
    let scene = scene_holding(Vec::new()).await;
    let source = SubscribingSource::start(&identifier(SOURCE)).await;

    scene
        .place_as_at_creation(&source.stored_without_subscription_creation())
        .await
        .expect("the placement is settled");

    pretty_assertions::assert_eq!(source.placed().await, Vec::<Value>::new());
    pretty_assertions::assert_eq!(scene.maps().claimed(), Vec::<ForwardedCopy>::new());
    pretty_assertions::assert_eq!(scene.router().registered(), Vec::<Identifier>::new());
}
