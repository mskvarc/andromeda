//! Lang outranks the accepted language.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::{Accept, AcceptLanguage},
        operation::QueryMethod,
    },
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: a lang parameter and the lang member of a Query body each outrank an Accept-Language naming another
/// language
/// Clause: 4.15, 5.7.1.4, 5.7.2.4
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0004_lang_outranks_the_accepted_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("fr".parse::<LanguageFilter>()?)?)?
        .entities()
        .retrieve(&building_id)
        .lang("nl".parse::<LanguageFilter>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("street", property().value("Grote Markt").lang("nl").without(Member::LanguageMap))
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("fr".parse::<LanguageFilter>()?)?)?
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .lang("nl".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            has_attribute("street", property().value("Grote Markt").lang("nl").without(Member::LanguageMap))
        )
    );
    Ok(())
}
