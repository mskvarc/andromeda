//! Update Attributes refuses location as a property and keeps the stored one.

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
    http::{UncheckedDocument, headers::Accept, operation::UpdateAttributes},
    model::{
        Entity,
        EntityFragment,
        ErrorType,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Update Attributes writing `location` as a Property onto an Entity holding it as a `GeoProperty`
/// answers 400 `BadRequestData` naming `location` and leaves the stored one, and updating it as a
/// `GeoProperty` answers 204 and moves it
/// Clause: 5.2.4, 5.5.4, 5.6.2.4
#[gtest]
#[tokio::test]
async fn n_att_upd_9011_update_attributes_refuses_location_as_a_property_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("GeoMemberProbe", "updated")?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(namespace.entity_type("GeoMemberProbe")?)
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    let empty = EntityFragment::builder().build()?;
    // The model admits `location` only as a GeoProperty, so it is written as text.
    let location: RawJson = r#"{"type": "Property", "value": "somewhere"}"#.parse()?;
    let request = broker
        .unchecked(&UpdateAttributes::builder().id(&probe_id).fragment(&empty).build())?
        .document(&UncheckedDocument::empty().set_member("location", &location));
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
    let moved = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_new(2.35, 48.85)?))
        .build()?;
    let request_2 = broker.unchecked(&UpdateAttributes::builder().id(&probe_id).fragment(&moved).build())?;
    expect_that!(&broker.send_unchecked(request_2).await, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = json.entities().retrieve(&probe_id).send().await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute("location", any_attribute().value_member(["coordinates", "0"], Value::Number("2.35".parse()?)))
    );
    Ok(())
}
