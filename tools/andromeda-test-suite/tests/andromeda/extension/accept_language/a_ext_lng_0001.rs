//! Retrieve entity answers the accepted language with its lang.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
    wire::vary,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::headers::{Accept, AcceptLanguage},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity with Accept-Language nl and no lang answers the street `LanguageProperty` as a Property
/// holding the Dutch string with lang nl, and the response varies on accept-language
/// Clause: 4.15, 5.7.1.4
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0001_retrieve_entity_answers_the_accepted_language_with_its_lang() -> Result<()> {
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
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("nl".parse::<LanguageFilter>()?)?)?
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(vary(headed.head.headers()), contains(eq(&HeaderName::from_static("accept-language"))));
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(attribute_members(entity, "street"), some(eq(3)));
    expect_that!(
        entity,
        has_attribute("street", property().value("Grote Markt").lang("nl").without(Member::LanguageMap))
    );
    Ok(())
}
