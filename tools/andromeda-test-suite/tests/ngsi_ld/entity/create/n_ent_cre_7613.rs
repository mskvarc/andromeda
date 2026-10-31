//! A sub attribute expires at with a numeric offset is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose Property holds a sub-Property carrying an `expiresAt` with a numeric offset in place
/// of Z (+02:00, or +00:00 naming UTC) answers 400 `BadRequestData`, and nothing is created
/// Clause: 4.6.3, 4.22, 5.6.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_7613_a_sub_attribute_expires_at_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    // The model refuses it (invalid timestamp at temperature.accuracy), so `temperature` is written as text.
    let temperature: RawJson =
        r#"{"type": "Property", "value": 21, "accuracy": {"type": "Property", "value": 0.5, "expiresAt": "2099-01-01T02:00:00+02:00"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("temperature", &temperature);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_2 = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    // The model refuses it (invalid timestamp at temperature.accuracy), so `temperature` is written as text.
    let temperature_2: RawJson =
        r#"{"type": "Property", "value": 21, "accuracy": {"type": "Property", "value": 0.5, "expiresAt": "2099-01-01T00:00:00+00:00"}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_2, Lossless::Normalized).set_member("temperature", &temperature_2);
    let request_2 = broker.unchecked(&CreateEntity::builder().entity(&building_2).build())?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
