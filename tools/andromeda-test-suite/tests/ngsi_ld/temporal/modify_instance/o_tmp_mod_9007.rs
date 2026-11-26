//! A modified instance holding a `valueType`, `objectType` or `vocab` the request's @context expands to no IRI is bad request data and changes nothing.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of, property, relationship},
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::ModifyAttributeInstance},
    model::{
        Attribute,
        AttributeKind,
        AttributeName,
        ErrorType,
        Input,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Relationship,
        VocabProperty,
        VocabTerm,
        evolution::TemporalEntity,
    },
};

/// Pins: Modify Attribute Instance whose @context defines the prefix ex, replacing the instance of the Property speed
/// by one whose valueType, the instance of the Relationship owner by one whose objectType or the instance of the
/// `VocabProperty` category by one whose vocab is `ex:a{b`, `ex:%`, `urn:x:{` or `https://example.org/{`, answers 400
/// `BadRequestData` for each, and the instances read back with the valueType <http://example.org/Kilometre>, the
/// objectType <http://example.org/Building> and the vocab <http://example.org/commercial>
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.14.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_tmp_mod_9007_a_modified_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let vehicle_id = namespace.urn("Vehicle", "modified")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let person_id = namespace.urn("Person", "o")?;
    let observed_at = "2020-08-01T12:03:00Z".parse()?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(1).value_type("ex:Kilometre".parse::<VocabTerm>()?).observed_at(observed_at),
        )
        .relationship(
            "owner",
            Relationship::builder(person_id.clone())
                .object_type("ex:Building".parse::<VocabTerm>()?)
                .observed_at(observed_at),
        )
        .vocab_property(
            "category",
            VocabProperty::builder("ex:commercial".parse::<VocabTerm>()?).observed_at(observed_at),
        )
        .build()?;
    writer.temporal().create_or_update(&vehicle).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let created = json.temporal().retrieve(&vehicle_id).send().await?;
    let placeholder: VocabTerm = "ex:Placeholder".parse()?;
    let families: [(&str, Attribute<Input>, &str); 3] = [
        (
            "speed",
            Attribute::from(Property::builder(1).value_type(placeholder.clone()).observed_at(observed_at).build()?),
            "valueType",
        ),
        (
            "owner",
            Attribute::from(
                Relationship::builder(person_id)
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
    let fragment = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    for value in UNEXPANDABLE_VALUES {
        for (name, attribute, member) in &families {
            let attribute_name: AttributeName = name.parse()?;
            let instance_id = instance_id_where(created.body(), &attribute_name, any_attribute())?;
            // The model refuses a type or a term that expands to no IRI, so the instance's member is set to it.
            let instance = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?.set_text(member, value);
            let broken: RawJson = format!("[{instance}]").parse()?;
            let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member(name, &broken);
            let request = writer
                .unchecked(
                    &ModifyAttributeInstance::builder()
                        .id(&vehicle_id)
                        .name(&attribute_name)
                        .instance_id(&instance_id)
                        .instance(attribute)
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
    let evolution = json.temporal().retrieve(&vehicle_id).send().await?;
    expect_that!(
        evolution.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(1)),
                contains(property().value(1).value_type("http://example.org/Kilometre".parse::<VocabTerm>()?))
            )
        )
    );
    expect_that!(
        evolution.body(),
        has_evolution(
            "owner",
            all!(
                len(eq(1)),
                contains(relationship().object_type("http://example.org/Building".parse::<VocabTerm>()?))
            )
        )
    );
    expect_that!(
        evolution.body(),
        has_evolution(
            "category",
            all!(
                len(eq(1)),
                contains(attribute_of(AttributeKind::VocabProperty).vocab("http://example.org/commercial".parse::<VocabTerm>()?))
            )
        )
    );
    Ok(())
}
