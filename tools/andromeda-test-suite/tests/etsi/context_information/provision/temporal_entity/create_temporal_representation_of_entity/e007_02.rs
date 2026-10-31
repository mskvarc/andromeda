//! A temporal representation of an entity is refused when its body is not valid JSON or is empty (clause 5.6.11).

use crate::{
    unchecked_requests::{post_empty, with_doubled_separator},
    vehicle_evolution_payloads::vehicle_speed_fuel_level_on,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, operation::CreateOrUpdateTemporalEvolution},
    model::{APPLICATION_LD_JSON, ErrorType},
};

/// Pins: Check that one cannot create a temporal entity with an empty/invalid json/id
/// Clause: 5.6.11
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e007_02_01_create_a_temporal_entity_with_an_invalid_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let client = broker.with_context_delivery(ContextDelivery::LdJsonBody);
    let evolution = vehicle_speed_fuel_level_on(&namespace, "8313926113148778", "2020-09-01")?;
    let operation = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    let request = client.unchecked(&operation)?.uri(api_uri(&client, "temporal/entities")?);
    let answer = client.send_unchecked(with_doubled_separator(request)).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    Ok(())
}

/// Pins: Check that one cannot create a temporal entity with an empty/invalid json/id
/// Clause: 5.6.11
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e007_02_02_create_a_temporal_entity_with_an_empty_json() -> Result<()> {
    let broker = broker::front()?;
    let answer = broker.send_unchecked(post_empty(&broker, "temporal/entities", &APPLICATION_LD_JSON)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    Ok(())
}
