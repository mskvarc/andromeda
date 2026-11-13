//! Replacing an entity with location as a vocab property is refused and keeps it.

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
    http::{UncheckedDocument, headers::Accept, operation::ReplaceEntity},
    model::{
        Entity,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Replace Entity with a body holding `location` as a `VocabProperty` answers 400 `BadRequestData` naming
/// `location` and leaves the stored Entity, and a body holding it as a `GeoProperty` answers 204 and replaces it
/// Clause: 5.2.4, 5.5.4, 5.6.18.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9010_replacing_an_entity_with_location_as_a_vocab_property_is_refused_and_keeps_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("GeoMemberProbe", "replaced")?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    let replacement = Entity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type)
        .geo_property("location", Shape::Point(Position::try_new(2.35, 48.85)?))
        .build()?;
    // The model admits `location` only as a GeoProperty, so it is written as text; Replace Entity names its target
    // in the path only, so the body carries no `id`.
    let location: RawJson = r#"{"type": "VocabProperty", "vocab": "somewhere"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&replacement, Lossless::Normalized)
        .remove_member("id")
        .set_member("location", &location);
    let request = broker.unchecked(&ReplaceEntity::builder().entity(&replacement).build())?.document(&document);
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
    let request_2 = broker.unchecked(&ReplaceEntity::builder().entity(&replacement).build())?;
    expect_that!(&broker.send_unchecked(request_2).await, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = json.entities().retrieve(&probe_id).send().await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute("location", any_attribute().value_member(["coordinates", "0"], Value::Number("2.35".parse()?)))
    );
    Ok(())
}
