//! Replacing location with a property is refused and keeps the stored one.

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
    http::{UncheckedDocument, headers::Accept, operation::ReplaceAttribute},
    model::{
        Attribute,
        AttributeName,
        Entity,
        ErrorType,
        GeoProperty,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Replace Attribute of `operationSpace` with a `JsonProperty` answers 400 `BadRequestData` naming
/// `operationSpace` and leaves the stored `GeoProperty`, and a `GeoProperty` answers 204 and replaces it
/// Clause: 5.2.4, 5.5.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_att_rpl_9005_replacing_operation_space_with_a_json_property_is_refused_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("GeoMemberProbe", "replaced")?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(namespace.entity_type("GeoMemberProbe")?)
        .geo_property("operationSpace", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    let operation_space_name: AttributeName = "operationSpace".parse()?;
    let replacement = Attribute::from(GeoProperty::builder(Shape::Point(Position::try_new(2.35, 48.85)?)).build()?);
    // The model admits `operationSpace` only as a GeoProperty, so the JsonProperty is written as text.
    let operation_space: RawJson = r#"{"type": "JsonProperty", "json": {"place": "somewhere"}}"#.parse()?;
    let request = broker
        .unchecked(
            &ReplaceAttribute::builder()
                .id(&probe_id)
                .name(&operation_space_name)
                .attribute(&replacement)
                .build(),
        )?
        .document(&UncheckedDocument::try_from(operation_space)?);
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("'operationSpace' must be a GeoProperty")
    );
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.entities().retrieve(&probe_id).send().await?;
    expect_that!(
        retrieved.body(),
        has_attribute(
            "operationSpace",
            any_attribute().value_member(["coordinates", "0"], Value::Number("14.5".parse()?))
        )
    );
    let request_2 = broker.unchecked(
        &ReplaceAttribute::builder()
            .id(&probe_id)
            .name(&operation_space_name)
            .attribute(&replacement)
            .build(),
    )?;
    expect_that!(&broker.send_unchecked(request_2).await, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = json.entities().retrieve(&probe_id).send().await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute(
            "operationSpace",
            any_attribute().value_member(["coordinates", "0"], Value::Number("2.35".parse()?))
        )
    );
    Ok(())
}
