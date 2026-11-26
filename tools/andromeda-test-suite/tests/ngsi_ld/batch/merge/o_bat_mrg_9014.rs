//! A batch merge item holding a `valueType`, `objectType` or `vocab` its @context expands to no IRI is a per-entity bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::reported,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, headers::Accept, operation::BatchMerge},
    model::{Attribute, Entity, EntityFragment, ErrorType, Input, Lossless, NgsiLdMediaType, Property, RawJson, Relationship, VocabProperty, VocabTerm},
};

/// Pins: Batch Entity Merge under an @context defining the prefix ex, setting temperature to 30 on two Buildings
/// holding 21, the first beside a Property speed whose valueType, a Relationship owner whose objectType or a
/// `VocabProperty` category whose vocab is `ex:a{b`, `ex:%`, `urn:x:{` or `https://example.org/{`, answers 207 with
/// `BadRequestData` for the first, which keeps 21 and holds no speed, owner or category, while the second reads 30
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.20.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_bat_mrg_9014_a_merge_value_type_object_type_or_vocab_that_expands_to_no_iri_is_a_per_entity_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let building_type = namespace.entity_type("Building")?;
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
            let refused_id = namespace.urn("Building", format!("refused-{position}"))?;
            let merged_id = namespace.urn("Building", format!("merged-{position}"))?;
            for id in [&refused_id, &merged_id] {
                let building = Entity::builder()
                    .id(id.clone())
                    .entity_type(building_type.clone())
                    .property("temperature", 21)
                    .build()?;
                broker.entities().create(&building).send().await?.done()?;
            }
            let fragments = [
                EntityFragment::for_entity(refused_id.clone(), building_type.clone())
                    .property("temperature", 30)
                    .attribute(*name, attribute.clone())
                    .build()?,
                EntityFragment::for_entity(merged_id.clone(), building_type.clone())
                    .property("temperature", 30)
                    .build()?,
            ];
            // The model refuses a type or a term that expands to no IRI, so the member is set to it.
            let broken: RawJson = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?
                .set_text(member, value)
                .to_string()
                .parse()?;
            let first = UncheckedDocument::try_from(fragments[0].render(Lossless::Normalized))?.set_member(name, &broken);
            let request = writer
                .unchecked(&BatchMerge::builder().fragments(&fragments).build())?
                .documents(&[first, UncheckedDocument::try_from(fragments[1].render(Lossless::Normalized))?]);
            let report: BatchOperationResult = reported(writer.send_unchecked(request).await)?;
            expect_that!(&report.errors, len(eq(1)), "{name} {value}");
            expect_that!(
                &report.errors,
                nth(0, batch_error().entity(&refused_id).problem(ErrorType::BadRequestData)),
                "{name} {value}"
            );
            expect_that!(&report.success, nth(0, eq(&merged_id)), "{name} {value}");
            let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
            let refused = json.entities().retrieve(&refused_id).send().await?;
            expect_that!(refused.body(), has_attribute("temperature", property().value(21)), "{name} {value}");
            expect_that!(refused.body(), lacks_attribute(name), "{name} {value}");
            let merged = json.entities().retrieve(&merged_id).send().await?;
            expect_that!(merged.body(), has_attribute("temperature", property().value(30)), "{name} {value}");
        }
    }
    Ok(())
}
