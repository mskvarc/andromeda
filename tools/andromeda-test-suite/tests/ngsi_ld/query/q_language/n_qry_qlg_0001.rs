//! Q filters the entities a type selects.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: q filters the entities a type selects: speed>50 keeps the fast Vehicle only, and `airQualityLevel==6` with
/// count=true counts the one matching Building
/// Clause: 4.9, 6.4.3.2, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0001_q_filters_the_entities_a_type_selects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_fast_id = namespace.urn("Vehicle", "fast")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_slow_id = namespace.urn("Vehicle", "slow")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(vehicle_fast_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 80)
            .build()?,
        Entity::builder()
            .id(vehicle_slow_id)
            .entity_type(vehicle_type.clone())
            .property("speed", 20)
            .build()?,
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("airQualityLevel", 4)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .property("airQualityLevel", 6)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_fast_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("airQualityLevel").eq(6))
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_2_id)));
    Ok(())
}
