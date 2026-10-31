//! Deleting the latest recorded instance leaves the entity value.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: of a speed a Core write set to 10 and then to 20, Delete Attribute instance naming the latest instance,
/// 20, answers 204, leaves Retrieve Entity answering 20 and Retrieve Temporal Evolution answering 10 alone; a later
/// Update Attributes setting 30 adds 30, and 20 never reappears
/// Clause: 4.3.1, 5.6.15, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, and clause 5.6.15 removes the instance from the
/// Temporal Evolution alone
#[gtest]
#[tokio::test]
async fn o_tmp_idl_9003_deleting_the_latest_recorded_instance_leaves_the_entity_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_trimmed_id = namespace.urn("Vehicle", "trimmed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_trimmed = Entity::builder()
        .id(vehicle_trimmed_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_trimmed).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(20).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_trimmed_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_trimmed_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    let latest = instance_id_where(evolution, &"speed".parse()?, any_attribute().value(20))?;
    broker
        .temporal()
        .delete_instance(&vehicle_trimmed_id, &"speed".parse::<AttributeName>()?, &latest)
        .send()
        .await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_trimmed_id)
        .send()
        .await?;
    let entity = retrieved_2.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(20)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_trimmed_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_2 = retrieved_3.body();
    expect_that!(evolution_2, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(10)))));
    let answer = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_trimmed_id, &"speed".parse::<AttributeName>()?, &latest)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let fragment_2 = EntityFragment::builder()
        .property("speed", Property::builder(30).observed_at("2024-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_trimmed_id, &fragment_2).send().await?.done()?;
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_trimmed_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_3 = retrieved_4.body();
    expect_that!(
        evolution_3,
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(10)), nth(1, any_attribute().value(30))))
    );
    Ok(())
}
