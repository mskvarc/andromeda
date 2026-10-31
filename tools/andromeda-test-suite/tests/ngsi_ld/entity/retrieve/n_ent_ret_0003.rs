//! Attrs naming no held attribute is resource not found.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity whose attrs names only attributes the entity does not hold is 404 `ResourceNotFound`
/// Clause: 5.7.1.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0003_attrs_naming_no_held_attribute_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .property(
            "airQualityLevel",
            Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
        )
        .property("almostFull", false)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .attrs(vec!["property_not_found".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .attrs(vec!["color".parse()?])
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // the entity itself is there
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .attrs(vec!["name".parse()?])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    Ok(())
}
