//! Clause C.6: how a time value is represented, and what may not be reified.
//!
//! The clause makes eight statements. Three are about the ways a reified
//! Property can carry a Date, `DateTime` or Time value; three are about what
//! JSON-LD type coercion does and does not do to such a Property; one bars
//! reification of a `TemporalProperty`; and one restricts which timestamps a
//! Temporal Query's `timeproperty` may name.

use crate::{BASE_URL, clean_tenant, client, comparison::corpus, context_link, create_entity, example_context::hosted_contexts, get_json};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;

/// Attempt one Entity creation and answer the status and body it produced.
async fn attempt_creation(entity: &Value, link: Option<&str>) -> (StatusCode, String) {
    let response = crate::write_request(&client(), &format!("{BASE_URL}/entities/"), entity, link)
        .send()
        .await
        .expect("entity creation request");
    let status = response.status();
    (status, response.text().await.unwrap_or_default())
}

#[tokio::test]
#[serial]
async fn annex_c_6_a_temporal_property_cannot_be_reified() {
    let environment = clean_tenant().await;

    // "a TemporalProperty does not allow reification": observedAt carries a
    // DateTime and nothing else, so a payload that gives it sub-Properties is
    // not a TemporalProperty at all.
    let reified = json!({
        "id": "urn:ngsi-ld:Vehicle:B9211", "type": "Vehicle",
        "speed": {"type": "Property", "value": 55,
                  "observedAt": {"type": "Property", "value": "2018-12-04T12:00:00Z",
                                 "accuracy": {"type": "Property", "value": 0.9}}}
    });
    let (status, body) = attempt_creation(&reified, None).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "clause C.6 bars reification of a TemporalProperty, but the broker accepted it: {body}"
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

#[tokio::test]
#[serial]
async fn annex_c_6_a_reified_property_carries_its_datatype_in_value_type() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    let printed = corpus(include_str!("corpus/c-6-value-type.json"));

    let (status, body) = attempt_creation(&printed, Some(&link)).await;
    assert_eq!(status, StatusCode::CREATED, "the C.6 valueType example was refused: {body}");

    let (status, actual) = get_json(&client(), "/entities/urn:ngsi-ld:Vehicle:B9211", "application/ld+json", Some(&link)).await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {actual}");
    assert_eq!(actual["testedAt"]["value"], json!("2018-12-04T12:00:00Z"));
    assert_eq!(
        actual["testedAt"]["valueType"],
        json!("DateTime"),
        "clause C.6 keeps the datatype in valueType, got {}",
        actual["testedAt"]
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

#[tokio::test]
#[serial]
async fn annex_c_6_a_reified_property_carries_its_datatype_in_a_typed_value() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    let printed = corpus(include_str!("corpus/c-6-typed-value.json"));

    let (status, body) = attempt_creation(&printed, Some(&link)).await;
    assert_eq!(status, StatusCode::CREATED, "the C.6 @value/@type example was refused: {body}");

    let (status, actual) = get_json(&client(), "/entities/urn:ngsi-ld:Vehicle:B9211", "application/ld+json", Some(&link)).await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {actual}");

    // Clause C.6's closing note: because `type` is mapped to `@type` in the
    // core @context, compaction renders the typed value with `type`, not
    // `@type`.
    assert_eq!(
        actual["testedAt"]["value"],
        json!({"type": "DateTime", "@value": "2018-12-04T12:00:00Z"}),
        "clause C.6 compacts @type to type inside the rendered value, got {}",
        actual["testedAt"]["value"]
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

#[tokio::test]
#[serial]
async fn annex_c_6_type_coercion_keeps_the_full_uri_in_the_normalized_representation() {
    let environment = clean_tenant().await;
    let coercing = context_link(hosted_contexts().url("tested-at-context.jsonld").as_str());

    // Written under the coercing @context, `testedAt` expands to
    // http://example.org/test/testedAt.
    let entity = json!({
        "id": "urn:ngsi-ld:Vehicle:B9211", "type": "Vehicle",
        "testedAt": {"type": "Property", "value": "2018-12-04T12:00:00Z"}
    });
    let (status, body) = attempt_creation(&entity, Some(&coercing)).await;
    assert_eq!(status, StatusCode::CREATED, "creation under the coercing @context was refused: {body}");

    let (status, actual) = get_json(&client(), "/entities/urn:ngsi-ld:Vehicle:B9211", "application/ld+json", Some(&coercing)).await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {actual}");
    assert!(
        actual.get("http://example.org/test/testedAt").is_some(),
        "clause C.6: a coerced term cannot compact a reified Property, so the full URI is kept; got {actual}"
    );
    assert!(
        actual.get("testedAt").is_none(),
        "clause C.6 states the short name is not used in the normalized representation; got {actual}"
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

#[tokio::test]
#[serial]
async fn annex_c_6_type_coercion_compacts_in_the_simplified_representation() {
    let environment = clean_tenant().await;
    let coercing = context_link(hosted_contexts().url("tested-at-context.jsonld").as_str());

    let entity = json!({
        "id": "urn:ngsi-ld:Vehicle:B9211", "type": "Vehicle",
        "testedAt": {"type": "Property", "value": "2018-12-04T12:00:00Z"}
    });
    let (status, body) = attempt_creation(&entity, Some(&coercing)).await;
    assert_eq!(status, StatusCode::CREATED, "creation under the coercing @context was refused: {body}");

    // "in the simplified (keyValue) representation case, such an @context with
    // the specification of testedAt could be used, as there is no reification."
    let (status, actual) = get_json(
        &client(),
        "/entities/urn:ngsi-ld:Vehicle:B9211?format=simplified",
        "application/ld+json",
        Some(&coercing),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {actual}");
    assert_eq!(
        actual["testedAt"],
        json!("2018-12-04T12:00:00Z"),
        "clause C.6: with no reification the coerced term compacts; got {actual}"
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

#[tokio::test]
#[serial]
async fn annex_c_6_only_a_structural_timestamp_can_be_a_temporal_query_time_property() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    create_entity(&client(), &corpus(include_str!("corpus/c-6-value-type.json")), Some(&link)).await;

    // Clause C.6, deferring to clause 4.11: the four structural timestamps are
    // the only admissible `timeproperty` values.
    for admissible in ["observedAt", "createdAt", "modifiedAt", "deletedAt"] {
        let path = format!("/temporal/entities/?type=Vehicle&timerel=after&timeAt=2018-01-01T00:00:00Z&timeproperty={admissible}");
        let (status, body) = get_json(&client(), &path, "application/ld+json", Some(&link)).await;
        assert_eq!(status, StatusCode::OK, "{admissible} is a structural timestamp: {body}");
    }

    let path = "/temporal/entities/?type=Vehicle&timerel=after&timeAt=2018-01-01T00:00:00Z&timeproperty=testedAt";
    let (status, body) = get_json(&client(), path, "application/ld+json", Some(&link)).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "clause 4.11 admits no user Property as timeproperty, but testedAt was accepted: {body}"
    );

    environment.reset_and_finish().await.expect("tenant reset");
}
