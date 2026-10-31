//! A batch upsert with a context nested in an attribute is a per entity bad request data.

use andromeda_test_suite::{
    broker,
    core_context::core_context,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution of an Entity refuses an attribute instance holding a nested user
/// @context with `BadRequestData` and stores no temporal entity, for a new entity and for an existing one
/// Clause: 5.5.7, 5.6.11
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_tmp_ups_2001_a_batch_upsert_with_a_context_nested_in_an_attribute_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device_evolution = TemporalEntity::builder().id(device_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson =
        r#"[{"type": "Property", "value": 1, "observedAt": "2020-01-01T00:00:00Z", "@context": {"speed": "https://example.org/speed"}}]"#.parse()?;
    let document = UncheckedDocument::try_from(device_evolution.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = core_context_body
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&device_evolution).build())?
        .document(&document);
    let answer = core_context_body.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let device_2_evolution = TemporalEntity::builder()
        .id(device_2_id.clone())
        .entity_type(device_type.clone())
        .property("speed", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&device_2_evolution).send().await?.created()?;
    let device_2_evolution_2 = TemporalEntity::builder().id(device_2_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed_2: RawJson =
        r#"[{"type": "Property", "value": 2, "observedAt": "2020-01-02T00:00:00Z", "@context": {"speed": "https://example.org/speed"}}]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(device_2_evolution_2.render(Lossless::Normalized))?.set_member("speed", &speed_2);
    let request_2 = core_context_body
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&device_2_evolution_2).build())?
        .document(&document_2);
    let answer_3 = core_context_body.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_2_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    Ok(())
}
