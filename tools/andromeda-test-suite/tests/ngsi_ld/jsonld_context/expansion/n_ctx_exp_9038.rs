//! Temporal writes refuse a scoped user context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution, Add Temporal Attributes and Modify Attribute Instance under a body
/// @context defining gear with a scoped @context each answer 400 `BadRequestData`, and the stored instance keeps
/// its value
/// Clause: 5.5.7, 5.6.11.4, 5.6.12.4, 5.6.14.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9038_temporal_writes_refuse_a_scoped_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_temporal_id = namespace.urn("Device", "temporal")?;
    let device_type = namespace.entity_type("Device")?;
    let core = config.core_context();
    let device_temporal_evolution = TemporalEntity::builder()
        .id(device_temporal_id.clone())
        .entity_type(device_type.clone())
        .property("gear", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    let answer = body_context
        .with_responses()
        .temporal()
        .create_or_update(&device_temporal_evolution)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_temporal_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let device_temporal_evolution_2 = TemporalEntity::builder()
        .id(device_temporal_id.clone())
        .entity_type(device_type.clone())
        .property("gear", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&device_temporal_evolution_2).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_temporal_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance = instance_id_at(evolution, &"gear".parse()?, 0)?;
    let instances = TemporalEntity::builder()
        .id(device_temporal_id.clone())
        .entity_type(device_type.clone())
        .property("gear", Property::builder(2).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?
        .attributes;
    let answer_3 = body_context
        .with_responses()
        .temporal()
        .add_attributes(&device_temporal_id, &instances)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The instance modification writes only one instance of the path's attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("temporal/entities/{device_temporal_id}/attrs/gear/{instance}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"gear\": [\n        {{\n            \"type\": \"Property\",\n            \"value\": 5\n        }}\n    ],\n    \"@context\": [\n        \"{core}\",\n        {{\n            \"gear\": {{\n                \"@id\": \"https://example.org/{namespace}/gear\",\n                \"@context\": {{\n                    \"observedAt\": \"https://example.org/{namespace}/rewritten\"\n                }}\n            }}\n        }}\n    ]\n}}")));
    let answer_4 = broker.send_unchecked(request).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_temporal_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("gear", all!(len(eq(1)), nth(0, any_attribute().value(1)))));
    Ok(())
}
