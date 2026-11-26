//! A batch upsert item holding a `valueType`, `objectType` or `vocab` its @context expands to no IRI is a per-entity bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    nth::nth,
    partial_outcome::reported,
    problem::answered,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchUpsert},
    model::{Attribute, Entity, ErrorType, Input, Lossless, NgsiLdMediaType, Property, RawJson, Relationship, VocabProperty, VocabTerm},
};

/// Pins: Batch Entity Upsert under an @context defining the prefix ex refuses the Vehicle holding a Property speed
/// whose valueType, a Relationship owner whose objectType or a `VocabProperty` category whose vocab is `ex:a{b`, `ex:%`,
/// `urn:x:{` or `https://example.org/{` with `BadRequestData` in the 207 answer and creates nothing of it, while the
/// Vehicle beside it is created
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.8.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_bat_ups_9025_a_value_type_object_type_or_vocab_that_expands_to_no_iri_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
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
            let refused_id = namespace.urn("Vehicle", format!("refused-{position}"))?;
            let created_id = namespace.urn("Vehicle", format!("created-{position}"))?;
            let entities = [
                Entity::builder()
                    .id(refused_id.clone())
                    .entity_type(vehicle_type.clone())
                    .attribute(*name, attribute.clone())
                    .build()?,
                Entity::builder()
                    .id(created_id.clone())
                    .entity_type(vehicle_type.clone())
                    .property("temperature", 2)
                    .build()?,
            ];
            // The model refuses a type or a term that expands to no IRI, so the member is set to it.
            let broken: RawJson = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?
                .set_text(member, value)
                .to_string()
                .parse()?;
            let first = UncheckedDocument::from_entity(&entities[0], Lossless::Normalized).set_member(name, &broken);
            let request = writer
                .unchecked(&BatchUpsert::builder().entities(&entities).build())?
                .documents(&[first, UncheckedDocument::from_entity(&entities[1], Lossless::Normalized)]);
            let report: BatchOperationResult = reported(writer.send_unchecked(request).await)?;
            expect_that!(&report.errors, len(eq(1)), "{name} {value}");
            expect_that!(
                &report.errors,
                nth(0, batch_error().entity(&refused_id).problem(ErrorType::BadRequestData)),
                "{name} {value}"
            );
            expect_that!(&report.success, nth(0, eq(&created_id)), "{name} {value}");
            let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
            let refused = json.with_responses().entities().retrieve(&refused_id).send().await;
            expect_that!(&refused, answered(StatusCode::NOT_FOUND), "{name} {value}");
            let created = json.entities().retrieve(&created_id).send().await?;
            expect_that!(created.body(), has_attribute("temperature", property().value(2)), "{name} {value}");
        }
    }
    Ok(())
}
