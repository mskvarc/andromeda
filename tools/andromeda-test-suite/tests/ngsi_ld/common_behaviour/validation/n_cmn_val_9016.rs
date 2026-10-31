//! Two value members on one attribute are refused.

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

/// Pins: Create Entity with a concise Attribute carrying both object and `languageMap` answers 400 `BadRequestData`
/// naming /both
/// Clause: 4.5.3.3, 4.5.18.3, 5.5.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9016_two_value_members_on_one_attribute_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at both), so `both` is written as text.
    let both: RawJson = format!(r#"{{"object": "urn:ngsi-ld:Thing:{namespace}-1", "languageMap": {{"en": "x"}}}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized).set_member("both", &both);
    let request = broker.unchecked(&CreateEntity::builder().entity(&device).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/both")
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
