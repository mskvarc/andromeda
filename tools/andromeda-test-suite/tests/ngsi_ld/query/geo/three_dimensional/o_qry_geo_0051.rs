//! Update attributes with a four ordinate position is bad request data.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateAttributes},
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

/// Pins: Update Attributes refuses a location with a four-ordinate position with `BadRequestData` and leaves the
/// stored three-ordinate position as it was
/// Clause: 4.7.1, 5.6.2, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 says implementations "SHOULD NOT extend positions beyond three elements" and
/// that parsers "MAY ignore" extra elements, and 4.7.1 requires a valid `GeoJSON` geometry without saying what a
/// fourth element is, so the specification does not require the refusal; Andromeda refuses a position of four
/// ordinates as `BadRequestData` (Table 5.5.2-1)
#[gtest]
#[tokio::test]
async fn o_qry_geo_0051_update_attributes_with_a_four_ordinate_position_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, 300.0, 9.0]}}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("location", &location);
    let request = broker
        .unchecked(&UpdateAttributes::builder().id(&building_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .value_member(["type"], "Point")
                .length_at(["coordinates"], 3)
                .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.52".parse()?))
                .value_member(["coordinates", "2"], 300)
        )
    );
    Ok(())
}
