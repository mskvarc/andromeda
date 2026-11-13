//! Append refuses observation space as a property and keeps the stored one.

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
    http::{UncheckedDocument, headers::Accept, operation::AppendAttributes},
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

/// Pins: Append Attributes writing `observationSpace` as a Property onto an Entity holding it as a `GeoProperty`
/// answers 400 `BadRequestData` naming `observationSpace` and leaves the stored one, and appending it as a
/// `GeoProperty` answers 204 and moves it
/// Clause: 5.2.4, 5.5.4, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_att_app_9014_append_refuses_observation_space_as_a_property_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("GeoMemberProbe", "appended")?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(namespace.entity_type("GeoMemberProbe")?)
        .geo_property("observationSpace", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    let empty = EntityFragment::builder().build()?;
    // The model admits `observationSpace` only as a GeoProperty, so it is written as text.
    let observation_space: RawJson = r#"{"type": "Property", "value": "somewhere"}"#.parse()?;
    let request = broker
        .unchecked(&AppendAttributes::builder().id(&probe_id).fragment(&empty).build())?
        .document(&UncheckedDocument::empty().set_member("observationSpace", &observation_space));
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("'observationSpace' must be a GeoProperty")
    );
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.entities().retrieve(&probe_id).send().await?;
    expect_that!(
        retrieved.body(),
        has_attribute(
            "observationSpace",
            any_attribute().value_member(["coordinates", "0"], Value::Number("14.5".parse()?))
        )
    );
    let moved = EntityFragment::builder()
        .geo_property("observationSpace", Shape::Point(Position::try_new(2.35, 48.85)?))
        .build()?;
    let request_2 = broker.unchecked(&AppendAttributes::builder().id(&probe_id).fragment(&moved).build())?;
    expect_that!(&broker.send_unchecked(request_2).await, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = json.entities().retrieve(&probe_id).send().await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute(
            "observationSpace",
            any_attribute().value_member(["coordinates", "0"], Value::Number("2.35".parse()?))
        )
    );
    Ok(())
}
