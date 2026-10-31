//! Every body other than the entity temporal fragment is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Modify Attribute instance with a bare instance, a bare array, an instance not contained by an array, two
/// instances, an empty array, two attributes, no attribute or another Entity's id answers 400 `BadRequestData`, and
/// the instance keeps 60
/// Clause: 5.4, 5.6.14.3, 5.6.14.4
#[gtest]
#[tokio::test]
async fn n_tmp_mod_9002_every_body_other_than_the_entity_temporal_fragment_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_fragment_refusals_id = namespace.urn("Vehicle", "fragment-refusals")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_fragment_refusals_evolution = TemporalEntity::builder()
        .id(vehicle_fragment_refusals_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .property("fuel", Property::builder(40).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_fragment_refusals_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_fragment_refusals_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance_id = instance_id_at(evolution, &"speed".parse()?, 0)?;
    expect_that!(evolution, has_evolution("speed", nth(0, any_attribute().with_member(Member::CreatedAt))));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request = request_to(
        &broker,
        Method::PATCH,
        &format!("temporal/entities/{vehicle_fragment_refusals_id}/attrs/speed/{instance_id}"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from(
        "{\n    \"type\": \"Property\",\n    \"value\": 129,\n    \"observedAt\": \"2025-01-01T00:00:00Z\"\n}",
    ));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_2 = request_to(
        &broker,
        Method::PATCH,
        &format!("temporal/entities/{vehicle_fragment_refusals_id}/attrs/speed/{instance_id}"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from(
        "[\n    {\n        \"type\": \"Property\",\n        \"value\": 129,\n        \"observedAt\": \"2025-01-01T00:00:00Z\"\n    }\n]",
    ));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_3 = request_to(
        &broker,
        Method::PATCH,
        &format!("temporal/entities/{vehicle_fragment_refusals_id}/attrs/speed/{instance_id}"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from(
        "{\n    \"speed\": {\n        \"type\": \"Property\",\n        \"value\": 129,\n        \"observedAt\": \"2025-01-01T00:00:00Z\"\n    }\n}",
    ));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_4 = request_to(&broker, Method::PATCH, &format!("temporal/entities/{vehicle_fragment_refusals_id}/attrs/speed/{instance_id}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n    \"speed\": [\n        {\n            \"type\": \"Property\",\n            \"value\": 129,\n            \"observedAt\": \"2025-01-01T00:00:00Z\"\n        },\n        {\n            \"type\": \"Property\",\n            \"value\": 129,\n            \"observedAt\": \"2025-01-01T00:00:00Z\"\n        }\n    ]\n}"));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_5 = request_to(
        &broker,
        Method::PATCH,
        &format!("temporal/entities/{vehicle_fragment_refusals_id}/attrs/speed/{instance_id}"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from("{\n    \"speed\": []\n}"));
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_6 = request_to(&broker, Method::PATCH, &format!("temporal/entities/{vehicle_fragment_refusals_id}/attrs/speed/{instance_id}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n    \"speed\": [\n        {\n            \"type\": \"Property\",\n            \"value\": 129,\n            \"observedAt\": \"2025-01-01T00:00:00Z\"\n        }\n    ],\n    \"fuel\": [\n        {\n            \"type\": \"Property\",\n            \"value\": 129,\n            \"observedAt\": \"2025-01-01T00:00:00Z\"\n        }\n    ]\n}"));
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_7 = request_to(
        &broker,
        Method::PATCH,
        &format!("temporal/entities/{vehicle_fragment_refusals_id}/attrs/speed/{instance_id}"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from("{}"));
    let answer_7 = broker.send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_8 = request_to(&broker, Method::PATCH, &format!("temporal/entities/{vehicle_fragment_refusals_id}/attrs/speed/{instance_id}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Vehicle:{namespace}-other\",\n    \"speed\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": 129,\n            \"observedAt\": \"2025-01-01T00:00:00Z\"\n        }}\n    ]\n}}")));
    let answer_8 = broker.send_unchecked(request_8).await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_fragment_refusals_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(60)))));
    Ok(())
}
