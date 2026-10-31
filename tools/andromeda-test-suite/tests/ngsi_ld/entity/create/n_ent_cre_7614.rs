//! A declared time value not of its form is bad request data at every depth.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity answers 400 `BadRequestData` for a value declared as a `DateTime`, a Date or a Time that is
/// not of its form, on a sub-attribute and inside a structured value as on an attribute, and nothing is created
/// Clause: 4.6.3, 4.5.2.2, 5.6.1, 5.5.4, C.6
#[gtest]
#[tokio::test]
async fn n_ent_cre_7614_a_declared_time_value_not_of_its_form_is_bad_request_data_at_every_depth() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid value at speed.calibratedAt), so `speed` is written as text.
    let speed: RawJson =
        r#"{"type": "Property", "value": 55, "calibratedAt": {"type": "Property", "value": {"@type": "DateTime", "@value": "2024-01-01T01:00:00+01:00"}}}"#
            .parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &speed);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_2 = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid value at speed.calibratedAt), so `speed` is written as text.
    let speed_2: RawJson =
        r#"{"type": "Property", "value": 55, "calibratedAt": {"type": "Property", "value": {"@type": "DateTime", "@value": "yesterday"}}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle_2, Lossless::Normalized).set_member("speed", &speed_2);
    let request_2 = broker.unchecked(&CreateEntity::builder().entity(&vehicle_2).build())?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_3 = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid value at service), so `service` is written as text.
    let service: RawJson = r#"{"type": "Property", "value": {"due": {"@type": "Date", "@value": "2024-1-1"}}}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&vehicle_3, Lossless::Normalized).set_member("service", &service);
    let request_3 = broker.unchecked(&CreateEntity::builder().entity(&vehicle_3).build())?.document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_4 = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (invalid value at opensAt), so `opensAt` is written as text.
    let opens_at: RawJson = r#"{"type": "Property", "value": {"@type": "Time", "@value": "08:00:00+00:00"}}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&vehicle_4, Lossless::Normalized).set_member("opensAt", &opens_at);
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
