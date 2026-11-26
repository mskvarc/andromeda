//! An appended `valueType`, `objectType` or `vocab` the request's @context expands to no IRI is bad request data.

use andromeda_test_suite::{
    broker,
    entity_matcher::lacks_attribute,
    example_prefix::{UNEXPANDABLE_VALUES, under_example_prefix},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AppendAttributes},
    model::{Attribute, Entity, EntityFragment, ErrorType, Input, Lossless, NgsiLdMediaType, Property, RawJson, Relationship, VocabProperty, VocabTerm},
};

/// Pins: Append Attributes whose @context defines the prefix ex, adding a Property speed whose valueType, a
/// Relationship owner whose objectType or a `VocabProperty` category whose vocab is `ex:a{b`, `ex:%`, `urn:x:{` or
/// `https://example.org/{`, answers 400 `BadRequestData` for each, and the Building holds no speed, owner or category
/// Clause: 4.5.2.2, 4.5.3.2, 4.6.2, 5.6.3.4
/// Recorded: Andromeda refuses a type or vocabulary term its @context expands to no absolute IRI, which would
/// otherwise be stored as a name that reads back as another
#[gtest]
#[tokio::test]
async fn o_att_app_9017_an_appended_value_type_object_type_or_vocab_that_expands_to_no_iri_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "appended")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
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
                .unchecked(&AppendAttributes::builder().id(&building_id).fragment(&fragment).build())?
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
    expect_that!(retrieved.body(), lacks_attribute("speed"));
    expect_that!(retrieved.body(), lacks_attribute("owner"));
    expect_that!(retrieved.body(), lacks_attribute("category"));
    Ok(())
}
