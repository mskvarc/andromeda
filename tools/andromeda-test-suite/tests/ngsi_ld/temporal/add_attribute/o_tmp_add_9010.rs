//! Restating the held instance keeps the entity modified at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Add Attributes to Temporal Evolution restating the speed value and `observedAt` a Vehicle holds answers
/// 204, adds the instance to the Temporal Evolution and leaves the entity-level and the speed `modifiedAt` as they
/// were
/// Clause: 4.5.5.3, 4.8, 5.8.6, 5.6.12.4
/// Recorded: clause 5.6.12.4 adds every instance to the Temporal Evolution and says nothing of current state;
/// Andromeda folds the instances into current state by the clause 4.5.5.3 rule, and an instance that differs from
/// the held one in no member (clause 5.8.6) modifies nothing there, so by clause 4.8 no `modifiedAt` moves
#[gtest]
#[tokio::test]
async fn o_tmp_add_9010_restating_the_held_instance_keeps_the_entity_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_restated_instance_id = namespace.urn("Vehicle", "restated-instance")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_restated_instance = Entity::builder()
        .id(vehicle_restated_instance_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_restated_instance).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_restated_instance_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_modified = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    let instances = TemporalEntity::builder()
        .id(vehicle_restated_instance_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&vehicle_restated_instance_id, &instances).send().await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_restated_instance_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::ModifiedAt)?, eq(entity_modified));
    expect_that!(system_time::of_attribute(entity_2, &["speed"], SystemMember::ModifiedAt)?, eq(speed_modified));
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(40)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_restated_instance_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    Ok(())
}
