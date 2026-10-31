//! A geometry collection geo property is bad request data.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, UpdateAttributes},
    },
    model::{
        Entity,
        EntityFragment,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: A `GeoProperty` whose value is a `GeometryCollection`, the one `GeoJSON` geometry the specification
/// excludes, is refused on create and on update, and nothing is stored
/// Clause: 4.7.1, 4.6.3, 5.5.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_0026_a_geometry_collection_geo_property_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_refused_id = namespace.urn("Spot", "refused")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_kept_id = namespace.urn("Spot", "kept")?;
    let spot_refused = Entity::builder().id(spot_refused_id.clone()).entity_type(spot_type.clone()).build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [13.4, 52.5]}, {"type": "LineString", "coordinates": [[13.1, 52.1], [13.3, 52.2]]}]}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&spot_refused, Lossless::Normalized).set_member("location", &location);
    let request = broker.unchecked(&CreateEntity::builder().entity(&spot_refused).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&spot_refused_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let spot_kept = Entity::builder()
        .id(spot_kept_id.clone())
        .entity_type(spot_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.5)?))
        .build()?;
    broker.entities().create(&spot_kept).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location_2: RawJson =
        r#"{"type": "GeoProperty", "value": {"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [14.1, 51.2]}]}}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("location", &location_2);
    let request_2 = broker
        .unchecked(&UpdateAttributes::builder().id(&spot_kept_id).fragment(&fragment).build())?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&spot_kept_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .value_member(["type"], "Point")
                .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.5".parse()?))
        )
    );
    Ok(())
}
