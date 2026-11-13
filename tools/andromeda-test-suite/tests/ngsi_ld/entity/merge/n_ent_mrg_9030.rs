//! A simplified merge gives a bare value the geo property type the entity holds.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::MergeSimplifiedEntity},
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        SimplifiedFragment,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Merge Entity with `format=simplified` writing `location` as a geometry encoded in a JSON string onto an
/// Entity holding it as a `GeoProperty` answers 204 and moves it, the `GeoProperty` type the Entity holds kept, while
/// writing `location` as a bare text onto an Entity holding none answers 400 `BadRequestData` naming `location` and
/// adds none
/// Clause: 5.2.4, 4.7.2, 4.5.2.3, 5.6.17.4, 6.5.3.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9030_a_simplified_merge_gives_a_bare_value_the_geo_property_type_the_entity_holds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    let located_id = namespace.urn("GeoMemberProbe", "located")?;
    let unlocated_id = namespace.urn("GeoMemberProbe", "unlocated")?;
    let located = Entity::builder()
        .id(located_id.clone())
        .entity_type(probe_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&located).send().await?.done()?;
    let unlocated = Entity::builder()
        .id(unlocated_id.clone())
        .entity_type(probe_type)
        .property("speed", 42)
        .build()?;
    broker.entities().create(&unlocated).send().await?.done()?;
    let placeholder = SimplifiedFragment::builder().value("speed", 1).build()?;
    // The model admits only a geometry object as a simplified `location`, so the encoded geometry is written as text.
    let request = broker
        .unchecked(&MergeSimplifiedEntity::builder().id(&located_id).fragment(&placeholder).build())?
        .document(&UncheckedDocument::empty().set_text("location", r#"{"type": "Point", "coordinates": [2.35, 48.85]}"#));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.entities().retrieve(&located_id).send().await?;
    expect_that!(
        retrieved.body(),
        has_attribute("location", any_attribute().value_member(["coordinates", "0"], Value::Number("2.35".parse()?)))
    );
    // The model admits only a geometry object as a simplified `location`, so the bare text is written as text.
    let request_2 = broker
        .unchecked(&MergeSimplifiedEntity::builder().id(&unlocated_id).fragment(&placeholder).build())?
        .document(&UncheckedDocument::empty().set_text("location", "nowhere"));
    expect_that!(
        &broker.send_unchecked(request_2).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("'location' must be a GeoProperty")
    );
    let retrieved_2 = json.entities().retrieve(&unlocated_id).send().await?;
    expect_that!(retrieved_2.body(), lacks_attribute("location"));
    Ok(())
}
