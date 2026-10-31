//! An entity created again after its deletion continues its temporal evolution.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Delete Entity of an Entity created with speed 30 observed at 2021-01-01 answers 204, and Create Entity
/// with the same id and speed 40 observed at 2021-02-01 answers 201; the Entity reads back with speed 40 and its
/// Temporal Evolution holds both instances
/// Clause: 5.6.6, 5.6.1, 4.3.1, 5.7.3
/// Recorded: clause 5.6.6 deletes the Entity and leaves its Temporal Evolution to clause 5.6.16; Andromeda keeps
/// the Temporal Evolution of a deleted Entity, so an Entity created again under the same id continues it
#[gtest]
#[tokio::test]
async fn o_tmp_del_9005_an_entity_created_again_after_its_deletion_continues_its_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_created_again_id = namespace.urn("Vehicle", "created-again")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_created_again = Entity::builder()
        .id(vehicle_created_again_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_created_again).send().await?.done()?;
    broker.entities().delete(&vehicle_created_again_id).send().await?.done()?;
    let vehicle_created_again_2 = Entity::builder()
        .id(vehicle_created_again_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2021-02-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_created_again_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_created_again_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(40)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_created_again_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                contains(any_attribute().value(30).observed_at("2021-01-01T00:00:00Z".parse()?)),
                contains(any_attribute().value(30)).times(eq(1)),
                contains(any_attribute().value(40).observed_at("2021-02-01T00:00:00Z".parse()?)),
                contains(any_attribute().value(40)).times(eq(1))
            )
        )
    );
    Ok(())
}
