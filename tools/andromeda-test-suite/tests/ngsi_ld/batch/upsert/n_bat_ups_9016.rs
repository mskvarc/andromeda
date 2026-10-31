//! An update upsert naming an entity twice holds the later and records both in order.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{Entity, NgsiLdMediaType},
    query::temporal_query::TemporalProperty,
};

/// Pins: Batch Entity Upsert with options=update naming a new Vehicle twice, with speed 2 and then speed 3, answers
/// 201; the Entity holds 3 and its Temporal Evolution holds 2 and then 3
/// Clause: 5.6.8, 5.5.11.0, 5.5.11.2, 5.7.3
#[gtest]
#[tokio::test]
async fn n_bat_ups_9016_an_update_upsert_naming_an_entity_twice_holds_the_later_and_records_both_in_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_batch_twice_id = namespace.urn("Vehicle", "batch-twice")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [
        Entity::builder()
            .id(vehicle_batch_twice_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 2)
            .build()?,
        Entity::builder()
            .id(vehicle_batch_twice_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 3)
            .build()?,
    ];
    broker.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_batch_twice_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(3)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_batch_twice_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(2)), nth(1, any_attribute().value(3))))
    );
    Ok(())
}
