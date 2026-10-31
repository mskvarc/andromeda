//! Queries and entity maps refuse an embedded context.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_context_list,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    received_status::ExpectedStatus,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Entity, ErrorType, NgsiLdMediaType},
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// The characters percent-encoding leaves as they are: the unreserved ones of IETF RFC 3986 section 2.3.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// Pins: Query Entities and Query Temporal Evolution of Entities via POST, and Create `EntityMap` via POST for
/// both, with an @context inside an entity selector answer 400 `BadRequestData`, and Update `EntityMap` with an
/// @context inside `linkedMaps` answers 400 `BadRequestData` and leaves `expiresAt` unchanged
/// Clause: 5.5.7, 5.7.2.4, 5.7.4.4, 5.14.2, 5.14.4, 6.34.3.2, 6.35
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9024_queries_and_entity_maps_refuse_an_embedded_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core = SuiteConfig::get()?.core_context();
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let query_body = r#"{
    "type": "Query",
    "entities": [
        {
            "type": "Device_{{ns}}",
            "@context": {
                "Device_{{ns}}": "https://example.org/{{ns}}/Device"
            }
        }
    ],
    "@context": [
        "{{core}}"
    ]
}"#
    .replace("{{ns}}", &namespace.to_string())
    .replace("{{core}}", core.as_str());
    // The model refuses an @context inside an entity selector, so every refused query is sent as written.
    for path in ["entityOperations/query", "temporal/entityOperations/query", "entityMaps", "temporal/entityMaps"] {
        let request = request_to(&broker, Method::POST, path)?
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
            .body(Bytes::from(query_body.clone()));
        let answer = broker.send_unchecked(request).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
        );
    }
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 1)
        .build()?;
    broker
        .with_context(core_context_list()?)?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&device)
        .send()
        .await?
        .done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&device_type)
        .ids(vec![device_id.clone()])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    let map_id = map.id.to_string();
    let patch_body = r#"{
    "expiresAt": "2099-01-01T00:00:00Z",
    "linkedMaps": {
        "@context": {}
    },
    "@context": [
        "{{core}}"
    ]
}"#
    .replace("{{core}}", core.as_str());
    // `linkedMaps` carrying an @context is no member the model writes, so the update is sent as written.
    let request_2 = request_to(&broker, Method::PATCH, &format!("entityMaps/{}", utf8_percent_encode(&map_id, UNRESERVED)))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(patch_body));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let kept = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map.id)
        .send()
        .await?;
    expect_that!(&kept.expires_at, eq(&map.expires_at));
    let deleted = broker.with_responses().entity_maps().delete(&map.id).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    Ok(())
}
