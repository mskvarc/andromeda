//! ETSI `007_03`: a temporal representation of an entity is refused when its id is missing or is not an IRI (clause
//! 5.6.11).

use crate::{
    suite_keywords::ld_json_body_client,
    suite_payloads::{VEHICLE_SPEED_FUEL_TERMS, vehicle_speed_fuel_level_with_dataset},
};
use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{api_uri, request_to},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, operation::CreateOrUpdateTemporalEvolution},
    model::Lossless,
};

/// Pins: Check that one cannot create a temporal entity with an invalid @context
/// Clause: 5.6.11
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e007_03_01_create_a_temporal_entity_with_missing_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let evolution = vehicle_speed_fuel_level_with_dataset(&namespace)?;
    let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.set_text("id", "");
    let client = broker.with_context_delivery(ContextDelivery::LdJsonBody);
    let operation = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    let request = client.unchecked(&operation)?.uri(api_uri(&client, "temporal/entities/")?);
    let answer = client.send_unchecked(request.document(&document)).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    broker
        .send_unchecked(request_to(&broker, Method::DELETE, "temporal/entities/")?)
        .await
        .any_answer();
    Ok(())
}

/// Pins: Check that one cannot create a temporal entity with an invalid @context
/// Clause: 5.6.11
#[gtest]
#[tokio::test]
async fn e007_03_02_create_a_temporal_invalid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS).await?;
    let evolution = vehicle_speed_fuel_level_with_dataset(&namespace)?;
    let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.set_text("id", "invalidId");
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let operation = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    let request = client.unchecked(&operation)?.uri(api_uri(&client, "temporal/entities/")?);
    let answer = client.send_unchecked(request.document(&document)).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    broker
        .send_unchecked(request_to(&broker, Method::DELETE, "temporal/entities/invalidId")?)
        .await
        .any_answer();
    Ok(())
}
