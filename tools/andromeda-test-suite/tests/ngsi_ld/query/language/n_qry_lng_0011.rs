//! Retrieve simplified without a language keeps the language map.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, SimplifiedValue},
};

/// Pins: Retrieve Entity with `options=keyValues` and no lang answers a `LanguageProperty` as an object holding its
/// `languageMap`
/// Clause: 4.5.4 EXAMPLE 4, 4.5.18
#[gtest]
#[tokio::test]
async fn n_qry_lng_0011_retrieve_simplified_without_a_language_keeps_the_language_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.attributes, len(eq(1)));
    expect_that!(
        entity.attributes.get(&"street".parse::<AttributeName>()?),
        some(eq(&SimplifiedValue::LanguageMap(LanguageMap::try_from_pairs([
            ("fr", "Grand Place"),
            ("nl", "Grote Markt")
        ])?)))
    );
    Ok(())
}
