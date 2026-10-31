//! A vocab property reads back as a vocab property.

use andromeda_test_suite::{attribute_matcher::attribute_of, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, EntityFragment, NgsiLdMediaType, VocabProperty, VocabTerm},
};

/// Pins: Append Attributes with a `VocabProperty` answers 204 and the Entity holds it as a `VocabProperty` with its
/// vocab
/// Clause: 5.6.3.4, 4.5.20
#[gtest]
#[tokio::test]
async fn n_att_app_0016_a_vocab_property_reads_back_as_a_vocab_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .vocab_property("vocabProperty", VocabProperty::builder("residential".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "vocabProperty",
            attribute_of(AttributeKind::VocabProperty).vocab("residential".parse::<VocabTerm>()?)
        )
    );
    Ok(())
}
