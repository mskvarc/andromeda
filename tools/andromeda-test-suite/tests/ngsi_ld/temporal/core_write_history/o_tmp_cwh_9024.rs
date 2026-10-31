//! An identical attribute update records no instance and keeps modified at.

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
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query::TemporalProperty,
};

/// Pins: Update Attributes restating the speed value and `observedAt` a Vehicle holds answers 204, leaves the
/// entity-level and the attribute-level `modifiedAt` as they were and adds no instance to the Temporal Evolution
/// Clause: 4.3.1, 4.8, 5.8.6, 5.6.2.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating ... whenever the current state is modified"); Andromeda reads a write that differs from the
/// stored instance in no member (clause 5.8.6) as no modification, so it records nothing and, by clause 4.8, moves
/// no `modifiedAt`
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9024_an_identical_attribute_update_records_no_instance_and_keeps_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_identical_update_id = namespace.urn("Vehicle", "identical-update")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_identical_update = Entity::builder()
        .id(vehicle_identical_update_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_identical_update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_identical_update_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_modified = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?;
    broker
        .entities()
        .update_attributes(&vehicle_identical_update_id, &fragment)
        .send()
        .await?
        .done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_identical_update_id)
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
        .retrieve(&vehicle_identical_update_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    Ok(())
}
