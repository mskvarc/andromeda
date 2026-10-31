//! New entities answer 201 listing every created id.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::{Done, UpsertCreated},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Upsert without options of entities that do not exist yet, whether they carry Properties,
/// Relationships or a Relationship on a Property, answers 201 with exactly the created ids, and the entities read
/// back
/// Clause: 5.6.8.4, 5.6.8.5, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0001_new_entities_answer_201_listing_every_created_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let building_4_id = namespace.urn("Building", 4)?;
    let city_pisa_id = namespace.urn("City", "Pisa")?;
    let building_5_id = namespace.urn("Building", 5)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let building_6_id = namespace.urn("Building", 6)?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("almostFull", false)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .build()?,
    ];
    let created = broker.entities().batch_upsert(&entities).send().await?.done()?.created_entities()?;
    expect_that!(created, len(eq(2)));
    expect_that!(created, contains(eq(&building_id)));
    expect_that!(created, contains(eq(&building_2_id)));
    let entities_2 = [
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id)
            .build()?,
        Entity::builder()
            .id(building_4_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_pisa_id.clone())
            .build()?,
    ];
    let created_2 = broker.entities().batch_upsert(&entities_2).send().await?.done()?.created_entities()?;
    expect_that!(created_2, len(eq(2)));
    expect_that!(created_2, contains(eq(&building_3_id)));
    expect_that!(created_2, contains(eq(&building_4_id)));
    let entities_3 = [
        Entity::builder()
            .id(building_5_id.clone())
            .entity_type(building_type.clone())
            .property("airQualityLevel", Property::builder(4).relationship("hasWeatherObserved", sensor_id))
            .build()?,
        Entity::builder()
            .id(building_6_id.clone())
            .entity_type(building_type.clone())
            .property("airQualityLevel", Property::builder(5).relationship("hasWeatherObserved", sensor_2_id.clone()))
            .build()?,
    ];
    let created_3 = broker.entities().batch_upsert(&entities_3).send().await?.done()?.created_entities()?;
    expect_that!(created_3, len(eq(2)));
    expect_that!(created_3, contains(eq(&building_5_id)));
    expect_that!(created_3, contains(eq(&building_6_id)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_4_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("locatedAt", any_attribute().object(city_pisa_id.clone())));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_6_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(
        entity_3,
        has_attribute(
            "airQualityLevel",
            any_attribute().sub_attribute("hasWeatherObserved", any_attribute().object(sensor_2_id.clone()))
        )
    );
    Ok(())
}
