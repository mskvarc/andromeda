//! ETSI `021_12`: a temporal evolution query that names neither an entity type nor an identifier is refused with 400
//! `BadRequestData` and a non-empty title (clause 5.7.4).

use crate::suite_keywords::{create_vehicle_speed_fuel_level, delete_temporal_entities, refused};
use andromeda_test_suite::{broker, namespace::Namespace, wire::request_to};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot query the temporal evolution of entities with an invalid request
/// Clause: 5.7.4
#[gtest]
#[tokio::test]
async fn e021_12_01_query_the_temporal_evolution_of_entities_with_an_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_speed_fuel_level(&broker, &namespace).await?;

    let client = broker.with_context(vocabulary.context())?;
    let request = request_to(&client, Method::GET, "temporal/entities?timerel=after&timeAt=2020-07-01T12%3A05%3A00Z")?;
    refused(&client, request, ErrorType::BadRequestData).await?;

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
