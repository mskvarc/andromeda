//! A sub attribute naming no attribute type is refused.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose speed carries an accuracy sub-Attribute typed Feature answers 400 `BadRequestData`
/// naming /speed/accuracy rather than keeping it as a value
/// Clause: 4.5.2.3, 5.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9012_a_sub_attribute_naming_no_attribute_type_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).build()?;
    // The model refuses it (unknown attribute type `Feature` at speed.accuracy), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 1, "accuracy": {"type": "Feature", "x": 1}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized).set_member("speed", &speed);
    let request = broker.unchecked(&CreateEntity::builder().entity(&device).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/speed/accuracy")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
