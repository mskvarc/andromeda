//! An expires at in the past is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Update `EntityMap` with an `expiresAt` in the past answers 400 `BadRequestData`
/// Clause: 5.14.2.4, 5.2.39
/// Recorded: 5.14.2.4 says nothing of an `expiresAt` already past; Andromeda refuses it as `BadRequestData`, as
/// 5.8.2 does for a Subscription
#[gtest]
#[tokio::test]
async fn o_map_upd_0007_an_expires_at_in_the_past_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
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
    let expires_at: DateTime = "2020-01-01T00:00:00Z".parse()?;
    let map_id = &created.body().id;
    let answer = broker.with_responses().entity_maps().update(map_id, &expires_at).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
