//! A geospatial member written as another attribute family is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
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

/// The members Table 5.2.4-1 types as `GeoProperty`.
const GEO_MEMBERS: [&str; 3] = ["location", "observationSpace", "operationSpace"];

/// One attribute of every family other than `GeoProperty`, and a concise Property, as JSON text.
const OTHER_FAMILIES: [&str; 8] = [
    r#"{"type": "Property", "value": "somewhere"}"#,
    r#"{"type": "Relationship", "object": "urn:ngsi-ld:Place:somewhere"}"#,
    r#"{"type": "LanguageProperty", "languageMap": {"en": "somewhere"}}"#,
    r#"{"type": "JsonProperty", "json": {"place": "somewhere"}}"#,
    r#"{"type": "VocabProperty", "vocab": "somewhere"}"#,
    r#"{"type": "ListProperty", "valueList": ["somewhere"]}"#,
    r#"{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Place:somewhere"]}"#,
    r#""somewhere""#,
];

/// Pins: Create Entity holding `location`, `observationSpace` or `operationSpace` as a Property, a Relationship, a
/// `LanguageProperty`, a `JsonProperty`, a `VocabProperty`, a `ListProperty`, a `ListRelationship` or a concise
/// Property answers 400 `BadRequestData` naming the member and stores nothing, and an Entity holding all three as
/// `GeoProperty` attributes is created
/// Clause: 5.2.4, 5.5.4, 5.6.1.4, 4.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_9063_a_geospatial_member_written_as_another_family_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    for member in GEO_MEMBERS {
        for (index, written) in OTHER_FAMILIES.into_iter().enumerate() {
            let probe = Entity::builder()
                .id(namespace.urn("GeoMemberProbe", format!("{member}-{index}"))?)
                .entity_type(probe_type.clone())
                .build()?;
            // The model admits the member only as a GeoProperty, so it is written as text.
            let attribute: RawJson = written.parse()?;
            let document = UncheckedDocument::from_entity(&probe, Lossless::Normalized).set_member(member, &attribute);
            let request = broker.unchecked(&CreateEntity::builder().entity(&probe).build())?.document(&document);
            let answer = broker.send_unchecked(request).await;
            expect_that!(
                &answer,
                problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing(&format!("'{member}' must be a GeoProperty")),
                "{member} as {written}"
            );
        }
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&probe_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let located = Entity::builder()
        .id(namespace.urn("GeoMemberProbe", "located")?)
        .entity_type(probe_type)
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .geo_property("observationSpace", Shape::Point(Position::try_new(14.6, 46.06)?))
        .geo_property("operationSpace", Shape::Point(Position::try_new(14.7, 46.07)?))
        .build()?;
    let request = broker.unchecked(&CreateEntity::builder().entity(&located).build())?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::CREATED));
    Ok(())
}
