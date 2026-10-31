//! A deleted entity leaves its recorded observations queryable.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Delete Entity of an Entity created with speed 55 observed at 2021-01-01 answers 204; the Entity is 404
/// `ResourceNotFound` and its Temporal Evolution still holds the speed instance 55
/// Clause: 5.6.6, 4.3.1, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9016_a_deleted_entity_leaves_its_recorded_observations_queryable() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_core_deleted_id = namespace.urn("Vehicle", "core-deleted")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_core_deleted = Entity::builder()
        .id(vehicle_core_deleted_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_core_deleted).send().await?.done()?;
    broker.entities().delete(&vehicle_core_deleted_id).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_core_deleted_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_core_deleted_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                contains(any_attribute().value(55).observed_at("2021-01-01T00:00:00Z".parse()?)),
                contains(any_attribute().value(55)).times(eq(1))
            )
        )
    );
    Ok(())
}
