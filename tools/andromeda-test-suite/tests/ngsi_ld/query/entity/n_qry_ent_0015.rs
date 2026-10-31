//! Twenty entities of every family read back in one page as written.

use andromeda_test_suite::{
    attribute_matcher::{AttributeMatcher, any_attribute, geo_property, property, relationship},
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        JsonProperty,
        LanguageMap,
        LanguageProperty,
        ListProperty,
        ListRelationship,
        NgsiLdMediaType,
        OneOrMany,
        Property,
        RawJson,
        Relationship,
        Value,
        VocabProperty,
        VocabTerm,
        geometry::{Position, Shape},
    },
};
use serde_json::json;

/// Pins: Query Entities answering one page of twenty entities of one type, each holding a Property with a
/// sub-attribute and one Attribute of one of the eight families, answers every entity member for member as Batch
/// Entity Creation wrote it
/// Clause: 5.6.7.4, 5.7.2.4, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_ent_0015_twenty_entities_of_every_family_read_back_in_one_page_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let person = "Person".parse::<VocabTerm>()?;
    let agent = "Agent".parse::<VocabTerm>()?;
    let mut entities = Vec::new();
    for i in 0..20_i32 {
        let parity = if i % 2 == 0 { "even" } else { "odd" };
        let base = Entity::builder()
            .id(namespace.urn("Building", format!("{i:02}"))?)
            .entity_type(building_type.clone())
            .property("index", Property::builder(i).property("parity", parity));
        entities.push(match i % 8 {
            0 => base
                .property(
                    "reading",
                    Property::builder(Value::Number(format!("{i}.5").parse()?))
                        .unit_code(UnitCode::Cel)
                        .observed_at("2026-01-01T00:00:00Z".parse()?),
                )
                .build()?,
            1 => base
                .relationship(
                    "owner",
                    Relationship::builder(vec![namespace.urn("Person", format!("{i}-2"))?, namespace.urn("Person", format!("{i}-1"))?])
                        .object_type(vec![person.clone(), agent.clone()]),
                )
                .build()?,
            2 => base
                .geo_property("location", Shape::Point(Position::try_new(10.25, f64::from(i) + 0.25)?))
                .build()?,
            3 => base
                .language_property(
                    "label",
                    LanguageProperty::builder(LanguageMap::try_from_pairs([("en", format!("label {i}")), ("de", format!("Etikett {i}"))])?),
                )
                .build()?,
            4 => base
                .vocab_property(
                    "category",
                    VocabProperty::builder(vec!["commercial".parse::<VocabTerm>()?, format!("office{i}").parse::<VocabTerm>()?]),
                )
                .build()?,
            5 => base
                .json_property(
                    "settings",
                    JsonProperty::builder(format!(r#"{{"level": {i}, "on": false}}"#).parse::<RawJson>()?),
                )
                .build()?,
            6 => base
                .list_property("series", ListProperty::builder(vec![Value::from(i), Value::from("two"), Value::from(true)]))
                .build()?,
            _ => base
                .list_relationship(
                    "route",
                    ListRelationship::builder(vec![namespace.urn("Stop", format!("{i}-2"))?, namespace.urn("Stop", format!("{i}-1"))?]),
                )
                .build()?,
        });
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(20)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(20)));
    for i in 0..20_i32 {
        let parity = if i % 2 == 0 { "even" } else { "odd" };
        let family: (&str, AttributeMatcher) = match i % 8 {
            0 => (
                "reading",
                property()
                    .value(Value::Number(format!("{i}.5").parse()?))
                    .unit_code(UnitCode::Cel)
                    .observed_at("2026-01-01T00:00:00Z".parse()?),
            ),
            1 => (
                "owner",
                relationship()
                    .object(OneOrMany::try_from(vec![
                        namespace.urn("Person", format!("{i}-2"))?,
                        namespace.urn("Person", format!("{i}-1"))?,
                    ])?)
                    .object_type(OneOrMany::try_from(vec![person.clone(), agent.clone()])?),
            ),
            2 => (
                "location",
                geo_property().geometry(Shape::Point(Position::try_new(10.25, f64::from(i) + 0.25)?)),
            ),
            3 => (
                "label",
                any_attribute().language("en", &format!("label {i}")).language("de", &format!("Etikett {i}")),
            ),
            4 => (
                "category",
                any_attribute().vocab(OneOrMany::try_from(vec![
                    "commercial".parse::<VocabTerm>()?,
                    format!("office{i}").parse::<VocabTerm>()?,
                ])?),
            ),
            5 => ("settings", any_attribute().json(json!({"level": i, "on": false}))),
            6 => (
                "series",
                any_attribute().list_length(3).list_value(0, i).list_value(1, "two").list_value(2, true),
            ),
            _ => (
                "route",
                any_attribute()
                    .list_length(2)
                    .list_object(0, namespace.urn("Stop", format!("{i}-2"))?)
                    .list_object(1, namespace.urn("Stop", format!("{i}-1"))?),
            ),
        };
        expect_that!(
            page.items(),
            contains(all!(
                has_id(&namespace.urn("Building", format!("{i:02}"))?),
                has_attribute("index", property().value(i).sub_attribute("parity", property().value(parity))),
                has_attribute(family.0, family.1)
            ))
        );
    }
    Ok(())
}
