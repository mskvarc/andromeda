//! An observed at in a reduced or lowercase spelling is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity answers 400 `BadRequestData` for an `observedAt` missing its seconds, missing its Z, written
/// with a lowercase t or z, or with a space for the T, and nothing is created
/// Clause: 4.6.3, 4.8, 5.6.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_7615_an_observed_at_in_a_reduced_or_lowercase_spelling_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00Z"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &speed);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_2 = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed_2: RawJson = r#"{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00"}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle_2, Lossless::Normalized).set_member("speed", &speed_2);
    let request_2 = broker.unchecked(&CreateEntity::builder().entity(&vehicle_2).build())?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_3 = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed_3: RawJson = r#"{"type": "Property", "value": 1, "observedAt": "2024-01-01t00:00:00z"}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&vehicle_3, Lossless::Normalized).set_member("speed", &speed_3);
    let request_3 = broker.unchecked(&CreateEntity::builder().entity(&vehicle_3).build())?.document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_4 = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid timestamp at speed), so `speed` is written as text.
    let speed_4: RawJson = r#"{"type": "Property", "value": 1, "observedAt": "2024-01-01 00:00:00Z"}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&vehicle_4, Lossless::Normalized).set_member("speed", &speed_4);
    let request_4 = broker.unchecked(&CreateEntity::builder().entity(&vehicle_4).build())?.document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
