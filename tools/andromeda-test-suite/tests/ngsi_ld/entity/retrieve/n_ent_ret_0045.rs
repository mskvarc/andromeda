//! A type selects the entity only when it has that type.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, Relationship},
};

/// Pins: Retrieve Entity with type naming the type of the Entity answers it, with another type answers 404
/// `ResourceNotFound`, and with a type selector ending in a dangling | answers 400 `BadRequestData`
/// Clause: 5.7.1.4, 4.17, 6.3.20
#[gtest]
#[tokio::test]
async fn n_ent_ret_0045_a_type_selects_the_entity_only_when_it_has_that_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_id));
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .entity_type(namespace.entity_type("Building")?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("entities/{vehicle_id}?type=Vehicle_{namespace}%7C"),
    )?;
    let answer_2 = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
