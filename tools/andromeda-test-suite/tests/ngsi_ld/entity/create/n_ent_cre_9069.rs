//! An `objectType` or `vocab` naming a prefixed term or an absolute IRI is stored as the IRI it expands to.

use andromeda_test_suite::{
    attribute_matcher::{attribute_of, relationship},
    broker,
    entity_matcher::has_attribute,
    example_prefix::under_example_prefix,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, NgsiLdMediaType, OneOrMany, Relationship, VocabProperty, VocabTerm},
};

/// Pins: Create Entity whose @context defines the prefix ex, with a Relationship owner whose objectType is
/// [ex:Building, urn:x-y] and a `VocabProperty` category whose vocab is ex:commercial, answers 201, and the Entity
/// retrieved under the core @context holds owner with the objectType [<http://example.org/Building>, urn:x-y] and
/// category with the vocab <http://example.org/commercial>
/// Clause: 4.5.3.2, 4.5.20, 4.6.2, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9069_an_object_type_or_vocab_naming_a_prefixed_term_or_an_iri_is_stored_expanded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let writer = under_example_prefix(&broker)?;
    let building_id = namespace.urn("Building", "typed-owner")?;
    let object_types = vec!["ex:Building".parse::<VocabTerm>()?, "urn:x-y".parse::<VocabTerm>()?];
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .relationship("owner", Relationship::builder(namespace.urn("Person", "o")?).object_type(object_types))
        .vocab_property("category", VocabProperty::builder("ex:commercial".parse::<VocabTerm>()?))
        .build()?;
    writer.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let expanded = OneOrMany::try_from(vec!["http://example.org/Building".parse::<VocabTerm>()?, "urn:x-y".parse::<VocabTerm>()?])?;
    expect_that!(retrieved.body(), has_attribute("owner", relationship().object_type(expanded)));
    expect_that!(
        retrieved.body(),
        has_attribute(
            "category",
            attribute_of(AttributeKind::VocabProperty).vocab("http://example.org/commercial".parse::<VocabTerm>()?)
        )
    );
    Ok(())
}
