//! A partial update to a `valueType`, `objectType` or `vocab` the request's @context expands to no IRI is bad request data.

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
    http::{UncheckedDocument, headers::Accept, operation::PartialAttributeUpdate},
    model::{
        Attribute,
        AttributeKind,
        AttributeName,
        Change,
        Entity,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        Patch,
        Property,
        Relationship,
        VocabProperty,
        VocabTerm,
    },
};

/// Pins: Partial Attribute Update whose @context defines the prefix ex, setting the valueType of the Property speed, the
/// objectType of the Relationship owner or the vocab of the `VocabProperty` category to `ex:a{b`, `ex:%`, `urn:x:{` or
/// `https://example.org/{`, answers 400 `BadRequestData` for each, and speed keeps the valueType
/// <http://example.org/Kilometre>, owner the objectType <http://example.org/Building> and category the vocab
/// <http://example.org/commercial>
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.4.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_att_pat_9029_a_partial_update_to_a_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let building_id = namespace.urn("Building", "patched")?;
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
    let families: [(&str, Attribute<Patch>, &str); 3] = [
        (
            "speed",
            Attribute::from(Property::builder(1).value_type(placeholder.clone()).build()?).into_patch(),
            "valueType",
        ),
        (
            "owner",
            Attribute::from(Relationship::builder(person_id).object_type(placeholder.clone()).build()?).into_patch(),
            "objectType",
        ),
        ("category", Attribute::from(VocabProperty::builder(placeholder).build()?).into_patch(), "vocab"),
    ];
    for value in UNEXPANDABLE_VALUES {
        for (name, patch, member) in &families {
            let attribute_name: AttributeName = name.parse()?;
            // The model refuses a type or a term that expands to no IRI, so the member is set to it.
            let document = UncheckedDocument::try_from(patch.render(Lossless::Normalized))?.set_text(member, value);
            let change = Change::from(patch.clone());
            let request = writer
                .unchecked(
                    &PartialAttributeUpdate::builder()
                        .id(&building_id)
                        .name(&attribute_name)
                        .attribute(&change)
                        .build(),
                )?
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
