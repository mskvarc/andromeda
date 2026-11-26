//! Added instances holding a `valueType`, `objectType` or `vocab` the request's @context expands to no IRI are bad request data and add nothing.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AddTemporalAttributes},
    model::{Attribute, ErrorType, Input, Lossless, NgsiLdMediaType, Property, RawJson, Relationship, VocabProperty, VocabTerm, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution whose @context defines the prefix ex, adding an instance of a Property
/// speed whose valueType, of a Relationship owner whose objectType or of a `VocabProperty` category whose vocab is
/// `ex:a{b`, `ex:%`, `urn:x:{` or `https://example.org/{`, answers 400 `BadRequestData` for each, and the Vehicle keeps
/// its one speed instance and holds no evolution of owner or category
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.12.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_tmp_add_9378_added_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    let writer = under_example_prefix(&broker)?;
    let placeholder: VocabTerm = "ex:Placeholder".parse()?;
    let observed_at = "2020-09-01T12:05:00Z".parse()?;
    let families: [(&str, Attribute<Input>, &str); 3] = [
        (
            "speed",
            Attribute::from(Property::builder(80).value_type(placeholder.clone()).observed_at(observed_at).build()?),
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
    for value in UNEXPANDABLE_VALUES {
        for (name, attribute, member) in &families {
            let instances = TemporalEntity::builder()
                .id(vehicle_id.clone())
                .entity_type(vehicle_type.clone())
                .attribute(*name, attribute.clone())
                .build()?
                .attributes;
            // The model refuses a type or a term that expands to no IRI, so the instance's member is set to it.
            let instance = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?.set_text(member, value);
            let broken: RawJson = format!("[{instance}]").parse()?;
            let document = UncheckedDocument::try_from(instances.render(Lossless::Normalized))?.set_member(name, &broken);
            let request = writer
                .unchecked(&AddTemporalAttributes::builder().id(&vehicle_id).attributes(&instances).build())?
                .document(&document);
            expect_that!(
                &writer.send_unchecked(request).await,
                problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
                "{name} {value}"
            );
        }
    }
    let evolution = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(evolution.body(), has_evolution("speed", len(eq(1))));
    expect_that!(evolution.body(), lacks_evolution("owner"));
    expect_that!(evolution.body(), lacks_evolution("category"));
    Ok(())
}
