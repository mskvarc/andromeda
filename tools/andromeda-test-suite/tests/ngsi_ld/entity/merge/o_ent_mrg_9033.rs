//! A merge to a `valueType`, `objectType` or `vocab` the request's @context expands to no IRI is bad request data and changes nothing.

use andromeda_test_suite::{
    attribute_matcher::{attribute_of, property, relationship},
    broker,
    entity_matcher::has_attribute,
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::MergeEntity},
    model::{
        Attribute,
        AttributeKind,
        Entity,
        EntityFragment,
        ErrorType,
        Input,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Relationship,
        VocabProperty,
        VocabTerm,
    },
};

/// Pins: Merge Entity whose @context defines the prefix ex, setting the valueType of the Property speed, the
/// objectType of the Relationship owner or the vocab of the `VocabProperty` category to `ex:a{b`, `ex:%`, `urn:x:{` or
/// `https://example.org/{`, answers 400 `BadRequestData` for each, and speed keeps the valueType
/// <http://example.org/Kilometre>, owner the objectType <http://example.org/Building> and category the vocab
/// <http://example.org/commercial>
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.17.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_ent_mrg_9033_a_merge_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let building_id = namespace.urn("Building", "merged")?;
    let person_id = namespace.urn("Person", "o")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .property("speed", Property::builder(1).value_type("ex:Kilometre".parse::<VocabTerm>()?))
        .relationship(
            "owner",
            Relationship::builder(person_id.clone()).object_type("ex:Building".parse::<VocabTerm>()?),
        )
        .vocab_property("category", VocabProperty::builder("ex:commercial".parse::<VocabTerm>()?))
        .build()?;
    writer.entities().create(&building).send().await?.done()?;
    let placeholder: VocabTerm = "ex:Placeholder".parse()?;
    let families: [(&str, Attribute<Input>, &str); 3] = [
        (
            "speed",
            Attribute::from(Property::builder(1).value_type(placeholder.clone()).build()?),
            "valueType",
        ),
        (
            "owner",
            Attribute::from(Relationship::builder(person_id).object_type(placeholder.clone()).build()?),
            "objectType",
        ),
        ("category", Attribute::from(VocabProperty::builder(placeholder).build()?), "vocab"),
    ];
    let fragment = EntityFragment::builder().build()?;
    for value in UNEXPANDABLE_VALUES {
        for (name, attribute, member) in &families {
            // The model refuses a type or a term that expands to no IRI, so the member is set to it.
            let broken: RawJson = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?
                .set_text(member, value)
                .to_string()
                .parse()?;
            let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member(name, &broken);
            let request = writer
                .unchecked(&MergeEntity::builder().id(&building_id).fragment(&fragment).build())?
                .document(&document);
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
    expect_that!(
        retrieved.body(),
        has_attribute("speed", property().value(1).value_type("http://example.org/Kilometre".parse::<VocabTerm>()?))
    );
    expect_that!(
        retrieved.body(),
        has_attribute("owner", relationship().object_type("http://example.org/Building".parse::<VocabTerm>()?))
    );
    expect_that!(
        retrieved.body(),
        has_attribute(
            "category",
            attribute_of(AttributeKind::VocabProperty).vocab("http://example.org/commercial".parse::<VocabTerm>()?)
        )
    );
    Ok(())
}
