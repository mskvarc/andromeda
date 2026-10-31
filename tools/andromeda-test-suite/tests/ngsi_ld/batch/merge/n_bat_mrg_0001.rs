//! A fragment without attributes keeps every attribute.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Merge whose fragments carry only id and type answers 204, and each entity keeps every
/// Attribute it held
/// Clause: 5.6.20.4, 5.6.20.5, 5.5.12
#[gtest]
#[tokio::test]
async fn n_bat_mrg_0001_a_fragment_without_attributes_keeps_every_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("airQualityLevel", Property::builder(4).unit_code(UnitCode::C62))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_id.clone(), building_type.clone()).build()?,
        EntityFragment::for_entity(building_2_id.clone(), building_type.clone()).build()?,
    ];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("airQualityLevel", any_attribute().value(4).unit_code(UnitCode::C62)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Pisa Tower")));
    Ok(())
}
