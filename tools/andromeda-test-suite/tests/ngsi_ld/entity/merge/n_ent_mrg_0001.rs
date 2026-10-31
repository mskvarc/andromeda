//! A fragment holding only id and type leaves the entity unchanged.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Property},
};

/// Pins: Merge Entity with a fragment that carries only the id and type answers 204 and leaves every Attribute of
/// the entity as it was
/// Clause: 5.6.17.4, 5.5.12, 6.5.3.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_0001_a_fragment_holding_only_id_and_type_leaves_the_entity_unchanged() -> Result<()> {
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
    let fragment = EntityFragment::for_entity(building_id.clone(), building_type.clone()).build()?;
    let received = broker.with_responses().entities().merge(&building_id, &fragment).send().await?;
    received.outcome.done()?;
    expect_that!(received.body.len(), eq(0));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("almostFull", any_attribute().value(false)));
    expect_that!(entity, has_attribute("airQualityLevel", any_attribute().value(4).unit_code(UnitCode::C62)));
    Ok(())
}
