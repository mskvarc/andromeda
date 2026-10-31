//! The latest instance has one modified at in both representations.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    instance_time::instance_time_where,
    namespace::Namespace,
    outcome::Done,
    system_time::{SystemMember, of_attribute},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: after Create Entity of a Sensor with temperature 20 observed at 2024-01-01 and Update Attributes setting
/// temperature 21 observed at 2024-01-02, Retrieve Entity with `options=sysAttrs` and Retrieve Temporal Evolution
/// of an Entity with `options=sysAttrs` give the latest temperature instance the same `createdAt` and `modifiedAt`
/// Clause: 4.3.1, 4.8, 5.6.3, 5.7.1, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, and clause 4.8 gives the one instance one
/// `createdAt` and one `modifiedAt`, whichever representation shows it
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9028_the_latest_instance_has_one_modified_at_in_both_representations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_one_instant_id = namespace.urn("Sensor", "one-instant")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_one_instant = Entity::builder()
        .id(sensor_one_instant_id.clone())
        .entity_type(sensor_type)
        .property("temperature", Property::builder(20).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&sensor_one_instant).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("temperature", Property::builder(21).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&sensor_one_instant_id, &fragment).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json
        .entities()
        .retrieve(&sensor_one_instant_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(21)));
    let core_created = *of_attribute(entity, &["temperature"], SystemMember::CreatedAt)?;
    let core_modified = *of_attribute(entity, &["temperature"], SystemMember::ModifiedAt)?;
    let retrieved_2 = json
        .temporal()
        .retrieve(&sensor_one_instant_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution, has_evolution("temperature", len(eq(2))));
    let latest = any_attribute().value(21);
    expect_that!(
        instance_time_where(evolution, &"temperature".parse()?, latest.clone(), SystemMember::ModifiedAt)?,
        eq(core_modified)
    );
    expect_that!(
        instance_time_where(evolution, &"temperature".parse()?, latest, SystemMember::CreatedAt)?,
        eq(core_modified)
    );
    expect_that!(
        instance_time_where(evolution, &"temperature".parse()?, any_attribute().value(20), SystemMember::CreatedAt)?,
        eq(core_created)
    );
    Ok(())
}
