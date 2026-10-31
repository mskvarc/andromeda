//! Count true answers the number of matching entities.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Query Entities with count=true answers NGSILD-Results-Count holding the number of matching entities, two
/// and then three after a third is created
/// Clause: 6.3.13, 4.13, 5.5.9, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_pag_0002_count_true_answers_the_number_of_matching_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
            )
            .property("almostFull", false)
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(6).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
            )
            .property("almostFull", true)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(2)));
    expect_that!(page.items(), len(eq(2)));
    let entities_2 = [Entity::builder()
        .id(building_3_id)
        .entity_type(building_type.clone())
        .property("name", "Louvre")
        .build()?];
    broker.entities().batch_create(&entities_2).send().await?.done()?;
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(3)));
    expect_that!(page_2.items(), len(eq(3)));
    Ok(())
}
