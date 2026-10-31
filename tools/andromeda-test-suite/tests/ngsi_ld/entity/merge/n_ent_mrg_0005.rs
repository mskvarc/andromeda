//! A value only fragment keeps the unit code and observed at.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Merge Entity with a Property that carries only a new value answers 204, replaces the value and keeps the
/// `unitCode` and `observedAt` the Property had
/// Clause: 5.6.17.4, 5.5.12, 6.5.3.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_0005_a_value_only_fragment_keeps_the_unit_code_and_observed_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
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
        .property("airQualityLevel", 1)
        .build()?;
    broker.entities().merge(&building_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "airQualityLevel",
            any_attribute().value(1).unit_code(UnitCode::C62).observed_at("2024-01-01T00:00:00Z".parse()?)
        )
    );
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    Ok(())
}
