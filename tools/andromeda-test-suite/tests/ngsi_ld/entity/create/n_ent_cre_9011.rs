//! A bbox of three numbers is refused.

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

/// Pins: Create Entity with a `GeoProperty` Point whose bbox holds three numbers answers 400 `BadRequestData` and
/// stores nothing, a bbox having 2*n numbers
/// Clause: 4.7.2, 5.5.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9011_a_bbox_of_three_numbers_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let parcel_id = namespace.urn("Parcel", 1)?;
    let parcel_type = namespace.entity_type("Parcel")?;
    let parcel = Entity::builder().id(parcel_id.clone()).entity_type(parcel_type).build()?;
    // The model refuses it (invalid geometry at area), so `area` is written as text.
    let area: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0], "bbox": [0.0, 0.0, 4.0]}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&parcel, Lossless::Normalized).set_member("area", &area);
    let request = broker.unchecked(&CreateEntity::builder().entity(&parcel).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&parcel_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
