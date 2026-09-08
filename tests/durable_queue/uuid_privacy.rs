//! What a broker-minted identifier discloses, and what it does not grant.
//!
//! Verification-matrix row "UUID privacy": every externally visible
//! broker-minted identifier is created and inspected. The temporal
//! `instanceId` clause 4.5.7 hands back is a version-7 UUID, so it discloses
//! the approximate instant it was minted — an intentional, documented property
//! of the identifier scheme, not an accident. What it must never be is a
//! capability: holding a valid identifier authorizes nothing about a resource
//! that does not own it.

use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::Utc;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

/// How far a minted identifier's embedded instant may sit from the wall clock.
const MINT_TIME_TOLERANCE_MILLISECONDS: i64 = 300_000;

/// A Vehicle whose single `speed` instance the broker must name itself.
fn vehicle(identifier: &str) -> Value {
    json!({
        "id": identifier,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"}],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// The `instanceId` the broker minted for the first instance of an attribute.
fn minted_instance_id(entity: &Value, attribute: &str) -> String {
    entity[attribute][0]["instanceId"]
        .as_str()
        .expect("a broker-minted instance identifier")
        .to_owned()
}

/// The UUID a rendered generated instance identifier is spelled from.
fn identifier_uuid(instance_id: &str) -> Uuid {
    let last = instance_id.rsplit(':').next().expect("a URN with a final segment");
    last.parse().expect("a broker-minted identifier is spelled from a UUID")
}

/// The Unix-millisecond timestamp a version-7 UUID carries in its first 48 bits.
fn minted_at_milliseconds(value: Uuid) -> i64 {
    let bytes = value.as_bytes();
    let mut timestamp = [0_u8; 8];
    timestamp[2..8].copy_from_slice(&bytes[0..6]);
    i64::try_from(u64::from_be_bytes(timestamp)).expect("a representable mint instant")
}

#[tokio::test]
async fn durable_queue_a_broker_minted_instance_identifier_discloses_only_its_mint_time() {
    let scope = TestEnvironment::from_suite("durablequeue").await.expect("isolated test environment");
    let identifier = scope.id("Vehicle:uuid-privacy-disclosure");
    let created = scope
        .client()
        .create_temporal_entity(&vehicle(&identifier))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "temporal create: {:?}", created.body);

    let retrieved = scope
        .client()
        .get_temporal_entity(&identifier, "", Some(COMPOUND_CONTEXT))
        .await
        .expect("retrieve temporal entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let instance_id = minted_instance_id(&retrieved.json_value().expect("temporal entity body"), "speed");

    assert!(
        instance_id.starts_with("urn:ngsi-ld:"),
        "a minted identifier is rendered through the broker's fixed URN template, got {instance_id}"
    );
    let value = identifier_uuid(&instance_id);
    assert_eq!(value.get_version_num(), 7, "minted instance identifiers are version-7 UUIDs");
    // RFC 9562 section 4.1: the variant bits of the ninth octet are 0b10xx.
    assert_eq!(value.as_bytes()[8] & 0b1100_0000, 0b1000_0000, "minted identifiers carry the RFC 9562 variant");

    let disclosed = minted_at_milliseconds(value);
    let now = Utc::now().timestamp_millis();
    assert!(
        (disclosed - now).abs() < MINT_TIME_TOLERANCE_MILLISECONDS,
        "the identifier discloses its approximate mint instant, which is the documented property of the scheme: \
         disclosed {disclosed}, now {now}"
    );

    scope.client().delete_temporal_entity(&identifier).await.expect("delete temporal entity failed");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn durable_queue_a_valid_instance_identifier_authorizes_nothing_on_another_entity() {
    let scope = TestEnvironment::from_suite("durablequeue").await.expect("isolated test environment");
    let owner = scope.id("Vehicle:uuid-privacy-owner");
    let other = scope.id("Vehicle:uuid-privacy-other");
    for identifier in [&owner, &other] {
        let created = scope
            .client()
            .create_temporal_entity(&vehicle(identifier))
            .await
            .expect("create temporal entity failed");
        assert_eq!(created.status, StatusCode::CREATED, "temporal create: {:?}", created.body);
    }

    let retrieved = scope
        .client()
        .get_temporal_entity(&owner, "", Some(COMPOUND_CONTEXT))
        .await
        .expect("retrieve temporal entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let instance_id = minted_instance_id(&retrieved.json_value().expect("temporal entity body"), "speed");

    let context = jsonld_link_header(COMPOUND_CONTEXT).expect("a valid JSON-LD Link header");
    let refused = scope
        .client()
        .raw(
            Method::DELETE,
            &format!(
                "/ngsi-ld/v1/temporal/entities/{}/attrs/speed/{}",
                urlencoding::encode(&other),
                urlencoding::encode(&instance_id)
            ),
        )
        .header("link", &context)
        .send()
        .await
        .expect("instance delete request failed");
    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "an identifier is not a capability: it names an instance of the Entity that owns it and nothing else"
    );

    let accepted = scope
        .client()
        .raw(
            Method::DELETE,
            &format!(
                "/ngsi-ld/v1/temporal/entities/{}/attrs/speed/{}",
                urlencoding::encode(&owner),
                urlencoding::encode(&instance_id)
            ),
        )
        .header("link", &context)
        .send()
        .await
        .expect("instance delete request failed");
    assert_eq!(
        accepted.status,
        StatusCode::NO_CONTENT,
        "the owning Entity's instance is addressable by the same identifier"
    );

    for identifier in [&owner, &other] {
        scope.client().delete_temporal_entity(identifier).await.expect("delete temporal entity failed");
    }
    scope.finish().await.expect("cleanup failed");
}
