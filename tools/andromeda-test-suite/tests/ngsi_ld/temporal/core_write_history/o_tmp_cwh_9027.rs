//! An identical replace entity records no instance and keeps modified at.

use andromeda_test_suite::{
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query::TemporalProperty,
};

/// Pins: Replace Entity restating a Vehicle exactly answers 204, leaves the entity-level and the attribute-level
/// `modifiedAt` as they were and adds no instance to the Temporal Evolution
/// Clause: 4.3.1, 4.8, 5.8.6, 5.6.18.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating ... whenever the current state is modified"); Andromeda reads a replacement that differs from
/// the stored Entity in no member (clause 5.8.6) as no modification, so it records nothing and, by clause 4.8,
/// moves no `modifiedAt`
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9027_an_identical_replace_entity_records_no_instance_and_keeps_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_identical_replace_id = namespace.urn("Vehicle", "identical-replace")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_identical_replace = Entity::builder()
        .id(vehicle_identical_replace_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_identical_replace).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_identical_replace_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_modified = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    let vehicle_identical_replace_replacement = Entity::builder()
        .id(vehicle_identical_replace_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?;
    broker.entities().replace(&vehicle_identical_replace_replacement).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_identical_replace_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::ModifiedAt)?, eq(entity_modified));
    expect_that!(system_time::of_attribute(entity_2, &["speed"], SystemMember::ModifiedAt)?, eq(speed_modified));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_identical_replace_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    Ok(())
}
