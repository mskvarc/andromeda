//! An unchanged value at a new observed at is recorded.

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
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: a Batch Entity Upsert restating the speed value a Vehicle holds with a later `observedAt` records a second
/// speed instance, at the new `observedAt`, and moves the attribute-level `modifiedAt`
/// Clause: 4.3.1, 5.8.6, 4.5.1, 4.8, 5.6.8.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating ... whenever the current state is modified"); `observedAt` is a member of the attribute's
/// node, so by clause 5.8.6 a new `observedAt` is a change even when the value is the same, and Andromeda records
/// it
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9025_an_unchanged_value_at_a_new_observed_at_is_recorded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_reobserved_id = namespace.urn("Vehicle", "reobserved")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_reobserved = Entity::builder()
        .id(vehicle_reobserved_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_reobserved).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_reobserved_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    let entities = [Entity::builder()
        .id(vehicle_reobserved_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2026-10-19T10:01:00Z".parse()?))
        .build()?];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_reobserved_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        system_time::of_attribute(entity_2, &["speed"], SystemMember::ModifiedAt)?,
        not(eq(speed_modified))
    );
    expect_that!(entity_2, has_attribute("speed", any_attribute().observed_at("2026-10-19T10:01:00Z".parse()?)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_reobserved_id)
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                contains(any_attribute().observed_at("2026-10-19T10:00:00Z".parse()?).value(40)),
                contains(any_attribute().observed_at("2026-10-19T10:00:00Z".parse()?)).times(eq(1)),
                contains(any_attribute().observed_at("2026-10-19T10:01:00Z".parse()?).value(40)),
                contains(any_attribute().observed_at("2026-10-19T10:01:00Z".parse()?)).times(eq(1))
            )
        )
    );
    Ok(())
}
