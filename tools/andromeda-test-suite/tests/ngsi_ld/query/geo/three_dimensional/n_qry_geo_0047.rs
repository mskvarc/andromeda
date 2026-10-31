//! Create with a single ordinate position is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity refuses a Point whose position has a single ordinate, and stores nothing
/// Clause: 4.7.1, 5.6.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_0047_create_with_a_single_ordinate_position_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4]}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("location", &location);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // Nothing was stored.
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
