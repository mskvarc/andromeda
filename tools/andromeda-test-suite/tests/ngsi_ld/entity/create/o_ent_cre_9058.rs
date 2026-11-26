//! A `valueType`, `objectType` or `vocab` the request's @context expands to no IRI is bad request data.

use andromeda_test_suite::{
    broker,
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Attribute, Entity, ErrorType, Input, Lossless, NgsiLdMediaType, Property, RawJson, Relationship, VocabProperty, VocabTerm},
};

/// Pins: Create Entity whose @context defines the prefix ex and whose Property speed has the valueType, whose
/// Relationship owner has the objectType or whose `VocabProperty` category has the vocab `ex:a{b`, `ex:%`, `urn:x:{` or
/// `https://example.org/{` answers 400 `BadRequestData` for each and creates nothing
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.1.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_ent_cre_9058_a_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
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
    let mut position = 0_u32;
    for value in UNEXPANDABLE_VALUES {
        for (name, attribute, member) in &families {
            position += 1;
            let vehicle_id = namespace.urn("Vehicle", position)?;
            let vehicle = Entity::builder()
                .id(vehicle_id.clone())
                .entity_type(namespace.entity_type("Vehicle")?)
                .build()?;
            // The model refuses a type or a term that expands to no IRI, so the member is set to it.
            let broken: RawJson = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?
                .set_text(member, value)
                .to_string()
                .parse()?;
            let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member(name, &broken);
            let request = writer.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
            expect_that!(
                &writer.send_unchecked(request).await,
                problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
                "{name} {value}"
            );
            let answer = broker
                .with_responses()
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .entities()
                .retrieve(&vehicle_id)
                .send()
                .await;
            expect_that!(&answer, answered(StatusCode::NOT_FOUND), "{name} {value}");
        }
    }
    Ok(())
}
