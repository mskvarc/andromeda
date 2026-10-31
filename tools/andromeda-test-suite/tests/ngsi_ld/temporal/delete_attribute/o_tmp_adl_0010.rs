//! A deleted core written evolution restarts with the next core write.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: of a speed a Core write set to 10 and then to 20, Delete Attribute from Temporal Evolution answers 204 and
/// leaves Retrieve Entity answering 20; a later Update Attributes setting 30 gives the Temporal Evolution the
/// single instance 30
/// Clause: 4.3.1, 5.6.17, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, and clause 5.6.17 removes the attribute from the
/// Temporal Evolution alone
#[gtest]
#[tokio::test]
async fn o_tmp_adl_0010_a_deleted_core_written_evolution_restarts_with_the_next_core_write() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_restarted_id = namespace.urn("Vehicle", "restarted")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_restarted = Entity::builder()
        .id(vehicle_restarted_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("brandName", Property::builder("Volvo").observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_restarted).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(20).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_restarted_id, &fragment).send().await?.done()?;
    broker
        .temporal()
        .delete_attribute(&vehicle_restarted_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_restarted_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(20)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_restarted_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution, lacks_evolution("speed"));
    expect_that!(evolution, has_evolution("brandName", len(eq(1))));
    let fragment_2 = EntityFragment::builder()
        .property("speed", Property::builder(30).observed_at("2024-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_restarted_id, &fragment_2).send().await?.done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_restarted_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_2 = retrieved_3.body();
    expect_that!(evolution_2, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(30)))));
    Ok(())
}
