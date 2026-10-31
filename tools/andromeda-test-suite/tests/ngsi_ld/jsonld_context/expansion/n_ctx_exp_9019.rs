//! Temporal writes refuse an embedded context.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_context_list,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, Property, RawJson, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution, Add Temporal Attributes and Modify Attribute Instance with an
/// @context inside an instance or a sub-Attribute answer 400 `BadRequestData`
/// Clause: 5.5.7, 5.6.11.4, 5.6.12.4, 5.6.14.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9019_temporal_writes_refuse_an_embedded_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let core = config.core_context();
    let device_evolution = TemporalEntity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    core_context_list_body.temporal().create_or_update(&device_evolution).send().await?.created()?;
    let device_evolution_2 = TemporalEntity::builder().id(device_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson = format!(
        r#"[{{"type": "Property", "value": 2, "observedAt": "2024-01-02T00:00:00Z", "@context": {{"speed": "https://example.org/{namespace}/speed"}}}}]"#
    )
    .parse()?;
    let document = UncheckedDocument::try_from(device_evolution_2.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = core_context_list_body
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&device_evolution_2).build())?
        .document(&document);
    let answer = core_context_list_body.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    // The model refuses this temporal fragment, so the body is sent as written.
    let request_2 = request_to(&broker, Method::POST, &format!("temporal/entities/{device_id}/attrs"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"gear\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": 3,\n            \"observedAt\": \"2024-01-02T00:00:00Z\",\n            \"@context\": {{}}\n        }}\n    ],\n    \"@context\": [\n        \"{core}\"\n    ]\n}}")));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    // The instance does not exist: the payload is judged first.
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request_3 = request_to(&broker, Method::PATCH, &format!("temporal/entities/{device_id}/attrs/speed/urn:ngsi-ld:AttributeInstance:{namespace}-probe"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"speed\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": 5,\n            \"accuracy\": {{\n                \"type\": \"Property\",\n                \"value\": 0.5,\n                \"@context\": {{}}\n            }}\n        }}\n    ],\n    \"@context\": [\n        \"{core}\"\n    ]\n}}")));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    Ok(())
}
