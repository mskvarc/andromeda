//! `expiresAt` semantics and duplicate-id handling (clause 5.9.2.4). A
//! registration without `expiresAt` persists indefinitely, a past `expiresAt` is
//! rejected, a future `expiresAt` deletes the registration once reached, and a
//! duplicate id is an `AlreadyExists` conflict. All `T-LOCAL`.

use crate::{
    oracle::{assert_problem_details, assert_status, json_body},
    reg::PEER_ENDPOINT,
    registration::csr_basic,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{ALREADY_EXISTS, BAD_REQUEST_DATA};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// How far ahead of "now" the near-future `expiresAt` is placed.
const EXPIRY_LEAD_SECONDS: i64 = 3;

/// How long to wait before asserting the registration is gone: the expiry lead
/// plus one full cadence of the broker's five-second expiry sweeper, with slack
/// for the sweep itself.
const EXPIRY_WAIT: Duration = Duration::from_secs(10);

/// A registration created without `expiresAt` persists indefinitely: it is stored
/// with no expiry and stays retrievable (clause 5.9.2.4). Confirms REQ-REG-59.
#[tokio::test]
async fn reg_without_expiresat_persists_indefinitely() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:no-expiry");
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&csr_basic(&id, PEER_ENDPOINT))
        .await
        .expect("create registration");
    assert_status(&created, 201);

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(&id, None)
        .await
        .expect("retrieve registration");
    assert_status(&retrieved, 200);
    assert!(
        json_body(&retrieved)["expiresAt"].is_null(),
        "a registration created without expiresAt carries no expiry"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A registration whose `expiresAt` is in the past is rejected (clause 5.9.2.4).
/// Confirms REQ-REG-60.
#[tokio::test]
async fn reg_past_expiresat_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:past-expiry");
    let mut body = csr_basic(&id, PEER_ENDPOINT);
    body["expiresAt"] = json!("2000-01-01T00:00:00Z");
    let rejected = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration with past expiry");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);

    topology.finish().await.expect("topology cleanup");
}

/// A registration with a near-future `expiresAt` is auto-deleted once the instant
/// is reached (clause 5.9.2.4: "If expiresAt is a date and time in the future,
/// implementations shall delete the Registration when this point in time is
/// reached"). The expiry is a few seconds ahead and the wait spans one full
/// sweeper cadence past it. Confirms REQ-REG-61.
#[tokio::test]
async fn reg_future_expiresat_auto_deleted_when_reached() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:future-expiry");
    let mut body = csr_basic(&id, PEER_ENDPOINT);
    body["expiresAt"] = json!((Utc::now() + ChronoDuration::seconds(EXPIRY_LEAD_SECONDS)).to_rfc3339());
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration with future expiry");
    assert_status(&created, 201);

    let before = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(&id, None)
        .await
        .expect("retrieve before expiry");
    assert_status(&before, 200);

    sleep(EXPIRY_WAIT).await;

    let after = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(&id, None)
        .await
        .expect("retrieve after expiry");
    assert_status(&after, 404);

    topology.finish().await.expect("topology cleanup");
}

/// Creating a second registration with an id that already exists is an
/// `AlreadyExists` conflict (clause 5.9.2.2). Confirms REQ-REG-62.
#[tokio::test]
async fn reg_duplicate_id_returns_409_alreadyexists() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:duplicate");
    let body = csr_basic(&id, PEER_ENDPOINT);
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration");
    assert_status(&created, 201);

    let duplicate = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create duplicate registration");
    assert_status(&duplicate, 409);
    assert_problem_details(&duplicate, ALREADY_EXISTS);

    topology.finish().await.expect("topology cleanup");
}
