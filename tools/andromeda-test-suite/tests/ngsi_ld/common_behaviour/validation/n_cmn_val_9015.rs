//! A geometry type without coordinates is refused.

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

/// Pins: Create Entity with an Attribute that is a bare {"type": "Point"} answers 400 `BadRequestData` naming /spot
/// Clause: 4.7.1, 4.5.2.3, 5.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9015_a_geometry_type_without_coordinates_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).build()?;
    // The model refuses it (unknown attribute type `Point` at spot), so `spot` is written as text.
    let spot: RawJson = r#"{"type": "Point"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized).set_member("spot", &spot);
    let request = broker.unchecked(&CreateEntity::builder().entity(&device).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/spot")
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
