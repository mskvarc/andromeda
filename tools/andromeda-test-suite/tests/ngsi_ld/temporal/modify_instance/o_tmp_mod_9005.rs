//! Modifying the latest recorded instance leaves the entity value.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: of a speed a Core write set to 10 and then to 20, Modify Attribute instance replacing the latest instance,
/// 20, by 25 answers 204, leaves Retrieve Entity answering 20, and leaves Retrieve Temporal Evolution answering 10
/// and 25 under the latest instance's `instanceId`; a later Update Attributes setting 30 then adds one instance,
/// 30, and 20 never reappears
/// Clause: 4.3.1, 4.5.7, 5.6.14, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, and clause 5.6.14 changes the Temporal Evolution
/// alone
#[gtest]
#[tokio::test]
async fn o_tmp_mod_9005_modifying_the_latest_recorded_instance_leaves_the_entity_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_modified_id = namespace.urn("Vehicle", "modified")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_modified = Entity::builder()
        .id(vehicle_modified_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_modified).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(20).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_modified_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_modified_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    let latest = instance_id_where(evolution, &"speed".parse()?, any_attribute().value(20))?;
    let instance = Attribute::from(Property::builder(25).observed_at("2024-01-02T00:00:00Z".parse()?).build()?);
    broker
        .temporal()
        .modify_instance(&vehicle_modified_id, &"speed".parse::<AttributeName>()?, &latest, &instance)
        .send()
        .await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_modified_id)
        .send()
        .await?;
    let entity = retrieved_2.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(20)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_modified_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_2 = retrieved_3.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                contains(any_attribute().value(10)),
                contains(any_attribute().value(25)),
                contains(any_attribute().value(25).instance_id(&latest)),
                contains(any_attribute().value(25)).times(eq(1))
            )
        )
    );
    let fragment_2 = EntityFragment::builder()
        .property("speed", Property::builder(30).observed_at("2024-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_modified_id, &fragment_2).send().await?.done()?;
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_modified_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_3 = retrieved_4.body();
    expect_that!(
        evolution_3,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                contains(any_attribute().value(10)),
                contains(any_attribute().value(25)),
                contains(any_attribute().value(30)),
                not(contains(any_attribute().value(20)))
            )
        )
    );
    Ok(())
}
