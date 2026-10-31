//! A context in the wrong body or none in a json ld body is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    received_status::ExpectedStatus,
    wire::content_type,
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateEntityMap},
    model::{APPLICATION_LD_JSON, DateTime, Entity, ErrorType, NgsiLdMediaType, Property, RawJson},
};

/// Pins: Update `EntityMap` answers 400 `BadRequestData` for an application/ld+json fragment without @context and
/// for an application/json fragment carrying one
/// Clause: 6.3.5, 5.14.2.4
#[gtest]
#[tokio::test]
async fn n_map_upd_0004_a_context_in_the_wrong_body_or_none_in_a_json_ld_body_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let plus_4h: DateTime = (Utc::now() + TimeDelta::hours(4)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let entities = [Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    let update = UpdateEntityMap {
        id: map_id,
        expires_at: &plus_4h,
    };
    // The typed update never sends an ld+json body without its context, so the media type is replaced.
    let without_context = UncheckedDocument::empty().set_text("expiresAt", &plus_4h.to_string());
    let request = content_type(broker.unchecked(&update)?.document_as_is(&without_context), &APPLICATION_LD_JSON)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The typed update never sends an application/json body with a context, so the member is added.
    let context: RawJson = format!(r#"["{}"]"#, config.core_context()).parse()?;
    let with_context = UncheckedDocument::empty()
        .set_member("@context", &context)
        .set_text("expiresAt", &plus_4h.to_string());
    let request_2 = broker.unchecked(&update)?.document_as_is(&with_context);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
