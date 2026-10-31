//! A new relationship is added beside the existing attributes.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, relationship},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Merge Entity with a fragment naming a Relationship the entity does not have answers 204, appends the
/// Relationship and keeps the existing Properties
/// Clause: 5.6.17.4, 5.5.12, 6.5.3.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_0004_a_new_relationship_is_added_beside_the_existing_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("almostFull", false)
        .property(
            "airQualityLevel",
            Property::builder(4).unit_code(UnitCode::C62).observed_at("2024-01-01T00:00:00.000Z".parse()?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::for_entity(building_id.clone(), building_type.clone())
        .relationship("locatedAt", city_paris_id.clone())
        .build()?;
    broker.entities().merge(&building_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("locatedAt", relationship().object(city_paris_id.clone())));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("almostFull", any_attribute().value(false)));
    expect_that!(entity, has_attribute("airQualityLevel", any_attribute().value(4)));
    Ok(())
}
