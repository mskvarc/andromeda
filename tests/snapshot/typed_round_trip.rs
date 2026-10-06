//! A Snapshot reads back as it was written, through its typed model, and opens
//! with its `@context` when served as `application/ld+json`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.15 and the Snapshot clauses 6.36 to 6.38 store
//! the members of the Snapshot data type a client supplies (`snapshotQueries`,
//! `snapshotLifetime`, `snapshotPriority`, `endpoint`) and serve them back.
//! Everything the broker computes is set aside from the comparison: the
//! `snapshotStatus`, the per-query execution details, and the `createdAt`,
//! `modifiedAt`, `expiresAt` and `lastUsedAt` timestamps. Clause 6.3.6 has an
//! `application/ld+json` response carry its `@context` in the body, and clause
//! 4.4 puts it first; the raw text is read because a parsed map keeps no order.
//! The Snapshot API has no collection retrieval, so there is no per-item case.
//!
//! Needs the host broker: the cases drive the HTTP API and are not run without
//! one.

use crate::support::{broker_url, clean_database, client, create_snapshot_raw, get_snapshot, wait_for_snapshot_terminal};
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;
use std::time::Duration;

/// The members the broker computes on a Snapshot.
const BROKER_COMPUTED_MEMBERS: [&str; 9] = [
    "@context",
    "snapshotStatus",
    "snapshotQueriesDetails",
    "snapshotTemporalQueriesDetails",
    "createdAt",
    "modifiedAt",
    "expiresAt",
    "lastUsedAt",
    "jsonldContext",
];

/// `snapshot` with the broker-computed members removed.
fn without_computed_members(mut snapshot: Value) -> Value {
    if let Some(members) = snapshot.as_object_mut() {
        for member in BROKER_COMPUTED_MEMBERS {
            members.remove(member);
        }
    }
    snapshot
}

/// The Snapshot every case writes.
fn posted_snapshot(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}],
        "snapshotLifetime": "PT1H",
        "snapshotPriority": 7
    })
}

/// Clauses 5.15 and 6.36 to 6.38: the Snapshot read back is the Snapshot
/// written, apart from the members the broker computes.
#[tokio::test]
#[serial(snapshot)]
async fn snapshot_round_trips_through_its_typed_model() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:typed-round-trip";
    let posted = posted_snapshot(id);

    let (status, _, problem) = create_snapshot_raw(&client(), &posted).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "create: {problem}");
    wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    let (status, returned) = get_snapshot(&client(), id).await;

    pretty_assertions::assert_eq!(status, StatusCode::OK, "retrieve: {returned}");
    pretty_assertions::assert_eq!(without_computed_members(returned), without_computed_members(posted));
}

/// Clauses 6.3.6 and 4.4: a Snapshot served as `application/ld+json` opens with
/// its `@context`.
#[tokio::test]
#[serial(snapshot)]
async fn snapshot_served_as_ld_json_opens_with_its_context() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:context-first";

    let (status, _, problem) = create_snapshot_raw(&client(), &posted_snapshot(id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "create: {problem}");
    wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    let response = client()
        .get(broker_url(&format!("/snapshots/{id}")))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("get Snapshot as ld+json request");
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    let text = response.text().await.expect("a Snapshot response body");

    assert!(
        text.trim_start()
            .strip_prefix('{')
            .is_some_and(|rest| rest.trim_start().starts_with("\"@context\"")),
        "a Snapshot opens with @context: {text}"
    );
}
