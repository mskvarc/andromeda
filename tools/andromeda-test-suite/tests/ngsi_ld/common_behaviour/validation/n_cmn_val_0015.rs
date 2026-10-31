//! Append attributes with an empty fragment changes nothing.

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
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::temporal_query::TemporalProperty,
};

/// Pins: Append Attributes with the empty Fragment {} as application/json answers 204 and changes nothing: every
/// value and `modifiedAt` stays as created, and the Temporal Evolution holds only the instances the creation
/// recorded
/// Clause: 5.4, 5.6.3.4, 6.6.3.1
/// Recorded: clause 5.4 makes a Fragment an IETF RFC 7396 JSON Merge Patch, under which the empty patch leaves its
/// target unchanged, and no operation lists an empty Fragment among its errors
#[gtest]
#[tokio::test]
async fn n_cmn_val_0015_append_attributes_with_an_empty_fragment_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_modified = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    let brand_modified = system_time::of_attribute(entity, &["brandName"], SystemMember::ModifiedAt)?;
    let fragment = EntityFragment::builder().build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::ModifiedAt)?, eq(entity_modified));
    expect_that!(system_time::of_attribute(entity_2, &["speed"], SystemMember::ModifiedAt)?, eq(speed_modified));
    expect_that!(
        system_time::of_attribute(entity_2, &["brandName"], SystemMember::ModifiedAt)?,
        eq(brand_modified)
    );
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(55)));
    expect_that!(entity_2, has_attribute("brandName", any_attribute().value("Mercedes")));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved_3.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    expect_that!(evolution, has_evolution("brandName", len(eq(1))));
    Ok(())
}
