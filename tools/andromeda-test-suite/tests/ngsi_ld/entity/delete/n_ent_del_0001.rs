//! A deleted entity is no longer found.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Delete Entity of an existing entity answers 204, after which Retrieve Entity of its id answers 404
/// `ResourceNotFound`
/// Clause: 5.6.6, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_del_0001_a_deleted_entity_is_no_longer_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .property(
            "airQualityLevel",
            Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    broker.entities().delete(&building_id).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
