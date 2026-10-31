//! A create answers 201 and the same id again already exists.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Location,
    model::{Entity, ErrorType},
};

/// Pins: Create Entity on broker A answers 201 with a Location ending in its id, and the same Create Entity again
/// answers 409 `AlreadyExists`
/// Clause: 5.6.1.4, 6.4.3.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_7600_a_create_answers_201_and_the_same_id_again_already_exists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    let received = broker.with_responses().entities().create(&vehicle).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(ends_with(format!("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:{namespace}-1")))
    );
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    let answer = broker.with_responses().entities().create(&vehicle_2).send().await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    Ok(())
}
