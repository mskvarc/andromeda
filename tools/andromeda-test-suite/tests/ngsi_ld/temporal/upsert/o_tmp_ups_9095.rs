//! A temporal `valueType`, `objectType` or `vocab` the request's @context expands to no IRI is bad request data.

use andromeda_test_suite::{
    broker,
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Attribute, ErrorType, Input, Lossless, NgsiLdMediaType, Property, RawJson, Relationship, VocabProperty, VocabTerm, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution whose @context defines the prefix ex and whose Property speed holds an
/// instance with the valueType, whose Relationship owner holds an instance with the objectType, or whose
/// `VocabProperty` category holds an instance with the vocab, `ex:a{b`, `ex:%`, `urn:x:{` or `https://example.org/{`
/// answers 400 `BadRequestData` for each and creates nothing
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.11.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9095_a_temporal_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let placeholder: VocabTerm = "ex:Placeholder".parse()?;
    let observed_at = "2020-08-01T12:03:00Z".parse()?;
    let families: [(&str, Attribute<Input>, &str); 3] = [
        (
            "speed",
            Attribute::from(Property::builder(1).value_type(placeholder.clone()).observed_at(observed_at).build()?),
            "valueType",
        ),
        (
            "owner",
            Attribute::from(
                Relationship::builder(namespace.urn("Person", "o")?)
                    .object_type(placeholder.clone())
                    .observed_at(observed_at)
                    .build()?,
            ),
            "objectType",
        ),
        (
            "category",
            Attribute::from(VocabProperty::builder(placeholder).observed_at(observed_at).build()?),
            "vocab",
        ),
    ];
    let mut position = 0_u32;
    for value in UNEXPANDABLE_VALUES {
        for (name, attribute, member) in &families {
            position += 1;
            let vehicle_id = namespace.urn("Vehicle", position)?;
            let evolution = TemporalEntity::builder()
                .id(vehicle_id.clone())
                .entity_type(namespace.entity_type("Vehicle")?)
                .build()?;
            // The model refuses a type or a term that expands to no IRI, so the instance's member is set to it.
            let instance = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?.set_text(member, value);
            let instances: RawJson = format!("[{instance}]").parse()?;
            let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.set_member(name, &instances);
            let request = writer
                .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build())?
                .document(&document);
            expect_that!(
                &writer.send_unchecked(request).await,
                problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
                "{name} {value}"
            );
            let answer = broker
                .with_responses()
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .temporal()
                .retrieve(&vehicle_id)
                .send()
                .await;
            expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound), "{name} {value}");
        }
    }
    Ok(())
}
