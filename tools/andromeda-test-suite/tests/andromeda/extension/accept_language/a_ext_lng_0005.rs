//! A wildcard accepts a language the property holds.

use andromeda_test_suite::{attribute_matcher::property, attribute_member::Member, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, AcceptLanguage},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity with Accept-Language * answers the street `LanguageProperty` as a Property holding one of
/// its strings with that string's lang
/// Clause: 4.15
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0005_a_wildcard_accepts_a_language_the_property_holds() -> Result<()> {
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
        .with_header(AcceptLanguage::try_from("*".parse::<LanguageFilter>()?)?)?
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    // The language chosen and the string shown are one pair: a French lang never carries the Dutch string.
    expect_that!(
        entity,
        any!(
            has_attribute("street", property().value("Grand Place").lang("fr").without(Member::LanguageMap)),
            has_attribute("street", property().value("Grote Markt").lang("nl").without(Member::LanguageMap))
        )
    );
    Ok(())
}
