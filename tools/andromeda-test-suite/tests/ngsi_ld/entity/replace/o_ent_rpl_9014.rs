//! A replacement holding a `valueType`, `objectType` or `vocab` the request's @context expands to no IRI is bad request data and changes nothing.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::ReplaceEntity},
    model::{Attribute, Entity, ErrorType, Input, Lossless, NgsiLdMediaType, Property, RawJson, Relationship, VocabProperty, VocabTerm},
};

/// Pins: Replace Entity whose @context defines the prefix ex, replacing a Building holding temperature 21 with one
/// holding temperature 30 beside a Property speed whose valueType, a Relationship owner whose objectType or a
/// `VocabProperty` category whose vocab is `ex:a{b`, `ex:%`, `urn:x:{` or `https://example.org/{`, answers 400
/// `BadRequestData` for each, and the Building keeps temperature 21 and holds no speed, owner or category
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.18.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_ent_rpl_9014_a_replacement_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "replaced")?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", 21)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let writer = under_example_prefix(&broker)?;
    let placeholder: VocabTerm = "ex:Placeholder".parse()?;
    let families: [(&str, Attribute<Input>, &str); 3] = [
        (
            "speed",
            Attribute::from(Property::builder(1).value_type(placeholder.clone()).build()?),
            "valueType",
        ),
        (
            "owner",
            Attribute::from(Relationship::builder(namespace.urn("Person", "o")?).object_type(placeholder.clone()).build()?),
            "objectType",
        ),
        ("category", Attribute::from(VocabProperty::builder(placeholder).build()?), "vocab"),
    ];
    for value in UNEXPANDABLE_VALUES {
        for (name, attribute, member) in &families {
            let replacement = Entity::builder()
                .id(building_id.clone())
                .entity_type(building_type.clone())
                .property("temperature", 30)
                .attribute(*name, attribute.clone())
                .build()?;
            // The model refuses a type or a term that expands to no IRI, so the member is set to it.
            let broken: RawJson = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?
                .set_text(member, value)
                .to_string()
                .parse()?;
            let document = UncheckedDocument::from_entity(&replacement, Lossless::Normalized).set_member(name, &broken);
            let request = writer.unchecked(&ReplaceEntity::builder().entity(&replacement).build())?.document(&document);
            expect_that!(
                &writer.send_unchecked(request).await,
                problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
                "{name} {value}"
            );
        }
    }
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("temperature", property().value(21)));
    expect_that!(retrieved.body(), lacks_attribute("speed"));
    expect_that!(retrieved.body(), lacks_attribute("owner"));
    expect_that!(retrieved.body(), lacks_attribute("category"));
    Ok(())
}
