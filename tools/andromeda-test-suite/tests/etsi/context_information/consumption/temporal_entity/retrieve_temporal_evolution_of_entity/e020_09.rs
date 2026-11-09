//! The temporal evolution of an entity is refused with 400 `BadRequestData` when its temporal query is incomplete: a
//! `timerel` without the `timeAt` it needs, or `between` without its `endTimeAt` (clause 5.7.3).

use crate::{unchecked_requests::refused_get, vehicle_fixtures::create_vehicle_speed_fuel_level};
use andromeda_test_suite::{broker, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;

/// Pins: Check that one cannot retrieve the temporal evolution of an entity with an invalid request content
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_09_01_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_speed_fuel_level(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    // The temporal query model cannot leave out the time it needs, so the query is sent as written.
    refused_get(&broker, &format!("temporal/entities/{id}?timerel=after")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot retrieve the temporal evolution of an entity with an invalid request content
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_09_02_before() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_speed_fuel_level(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    // The temporal query model cannot leave out the time it needs, so the query is sent as written.
    refused_get(&broker, &format!("temporal/entities/{id}?timerel=before")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one cannot retrieve the temporal evolution of an entity with an invalid request content
/// Clause: 5.7.3
#[gtest]
#[tokio::test]
async fn e020_09_03_between() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_speed_fuel_level(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    // The temporal query model cannot leave out the time it needs, so the query is sent as written.
    refused_get(&broker, &format!("temporal/entities/{id}?timerel=between&timeAt=2020-08-01T12%3A00%3A00Z")).await?;

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
