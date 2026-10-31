//! A type that is not a string is refused.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Create Entity with an Attribute typed 7 answers 400 `BadRequestData` naming /raw, and with a sub-Attribute
/// typed 7 answers 400 `BadRequestData` naming /speed/accuracy
/// Clause: Table 5.2.5-1, 4.5.2.3, 5.5.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9014_a_type_that_is_not_a_string_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device = Entity::builder().id(device_id).entity_type(device_type.clone()).build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at raw), so `raw` is written as text.
    let raw: RawJson = r#"{"type": 7, "value": 1}"#.parse()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized).set_member("raw", &raw);
    let request = broker.unchecked(&CreateEntity::builder().entity(&device).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/raw")
    );
    let device_2 = Entity::builder().id(device_2_id).entity_type(device_type.clone()).build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at speed.accuracy), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 1, "accuracy": {"type": 7, "value": 0.5}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&device_2, Lossless::Normalized).set_member("speed", &speed);
    let request_2 = broker.unchecked(&CreateEntity::builder().entity(&device_2).build())?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/speed/accuracy")
    );
    Ok(())
}
