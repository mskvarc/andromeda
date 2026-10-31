//! Create with a four ordinate custom geo property is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{
        Entity,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
};

/// Pins: Create Entity refuses an entity whose valid location sits beside an `observationPoint` with a
/// four-ordinate position, and stores nothing
/// Clause: 4.7.1, 5.6.1, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 says implementations "SHOULD NOT extend positions beyond three elements" and
/// that parsers "MAY ignore" extra elements, and 4.7.1 requires a valid `GeoJSON` geometry without saying what a
/// fourth element is, so the specification does not require the refusal; Andromeda refuses a position of four
/// ordinates as `BadRequestData` (Table 5.5.2-1)
#[gtest]
#[tokio::test]
async fn o_qry_geo_0050_create_with_a_four_ordinate_custom_geo_property_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 10.0)?))
        .build()?;
    // The model refuses it (invalid geometry at observationPoint), so `observationPoint` is written as text.
    let observation_point: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, 10.0, 1.0]}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("observationPoint", &observation_point);
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
