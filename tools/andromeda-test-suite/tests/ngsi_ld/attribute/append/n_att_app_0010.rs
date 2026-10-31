//! A language property reads back with its language map.

use andromeda_test_suite::{attribute_matcher::attribute_of, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, EntityFragment, LanguageMap, LanguageProperty, NgsiLdMediaType},
};

/// Pins: Append Attributes with a `LanguageProperty` answers 204 and the Entity holds it with its `languageMap`
/// Clause: 5.6.3.4, 4.5.18
#[gtest]
#[tokio::test]
async fn n_att_app_0010_a_language_property_reads_back_with_its_language_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Grand Place"), ("fr", "Grand Place")])?),
        )
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
            "street",
            attribute_of(AttributeKind::LanguageProperty)
                .language("fr", "Grand Place")
                .language("en", "Grand Place")
        )
    );
    Ok(())
}
