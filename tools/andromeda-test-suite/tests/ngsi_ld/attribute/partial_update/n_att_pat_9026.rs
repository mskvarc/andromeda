//! A partial update of location as a property is refused and keeps the stored one.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::PartialAttributeUpdate},
    model::{
        Attribute,
        AttributeName,
        Change,
        Entity,
        ErrorType,
        GeoProperty,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Partial Attribute Update of `location` with a Property fragment answers 400 `BadRequestData` naming
/// `location` and leaves the stored `GeoProperty`, and a `GeoProperty` fragment answers 204 and moves it
/// Clause: 5.2.4, 5.5.4, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9026_a_partial_update_of_location_as_a_property_is_refused_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("GeoMemberProbe", "patched")?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(namespace.entity_type("GeoMemberProbe")?)
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    let location_name: AttributeName = "location".parse()?;
    let moved = Change::from(Attribute::from(GeoProperty::builder(Shape::Point(Position::try_new(2.35, 48.85)?)).build()?).into_patch());
    // The model admits `location` only as a GeoProperty, so the Property fragment is written as text.
    let location: RawJson = r#"{"type": "Property", "value": "somewhere"}"#.parse()?;
    let request = broker
        .unchecked(&PartialAttributeUpdate::builder().id(&probe_id).name(&location_name).attribute(&moved).build())?
        .document(&UncheckedDocument::try_from(location)?);
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("'location' must be a GeoProperty")
    );
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.entities().retrieve(&probe_id).send().await?;
    expect_that!(
        retrieved.body(),
        has_attribute("location", any_attribute().value_member(["coordinates", "0"], Value::Number("14.5".parse()?)))
    );
    let request_2 = broker.unchecked(&PartialAttributeUpdate::builder().id(&probe_id).name(&location_name).attribute(&moved).build())?;
    expect_that!(&broker.send_unchecked(request_2).await, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = json.entities().retrieve(&probe_id).send().await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute("location", any_attribute().value_member(["coordinates", "0"], Value::Number("2.35".parse()?)))
    );
    Ok(())
}
