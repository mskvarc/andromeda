//! Older instances added to a core written attribute leave its value.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: of a speed a Core write set to 50 observed at 2024-01-10, Create or Update Temporal Evolution adding
/// instances 1 and 2 observed at 2024-01-01 and 2024-01-02 answers 204, leaves Retrieve Entity answering 50, and
/// Retrieve Temporal Evolution answers 1, 2 and 50 in `observedAt` order
/// Clause: 4.3.1, 5.6.11, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, and clause 5.6.11 adds the instances to the
/// Temporal Evolution
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9028_older_instances_added_to_a_core_written_attribute_leave_its_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_backfilled_id = namespace.urn("Vehicle", "backfilled")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_backfilled = Entity::builder()
        .id(vehicle_backfilled_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2024-01-10T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_backfilled).send().await?.done()?;
    let vehicle_backfilled_evolution = TemporalEntity::builder()
        .id(vehicle_backfilled_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_backfilled_evolution).send().await?.updated()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_backfilled_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(50)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_backfilled_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                nth(0, any_attribute().value(1)),
                nth(1, any_attribute().value(2)),
                nth(2, any_attribute().value(50))
            )
        )
    );
    Ok(())
}
