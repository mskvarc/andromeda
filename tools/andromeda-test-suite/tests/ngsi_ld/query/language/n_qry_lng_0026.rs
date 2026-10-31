//! Retrieve and query without lang ignore accept language.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
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
    model::{AttributeKind, Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Retrieve Entity and Query Entities by GET and by POST carrying Accept-Language nl and no lang answer the
/// street `LanguageProperty` with its whole `languageMap`, because the language filter is the lang parameter alone
/// Clause: 4.15, 4.5.18, 5.7.1.4, 5.7.2.4, Table 6.5.3.1-1, Table 6.4.3.2-1
#[gtest]
#[tokio::test]
async fn n_qry_lng_0026_retrieve_and_query_without_lang_ignore_accept_language() -> Result<()> {
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
        .with_header(AcceptLanguage::try_from("nl".parse::<LanguageFilter>()?)?)?
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "street",
            attribute_of(AttributeKind::LanguageProperty)
                .language("fr", "Grand Place")
                .language("nl", "Grote Markt")
                .without(Member::Lang)
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("nl".parse::<LanguageFilter>()?)?)?
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            has_attribute(
                "street",
                attribute_of(AttributeKind::LanguageProperty)
                    .language("fr", "Grand Place")
                    .language("nl", "Grote Markt")
                    .without(Member::Lang)
            )
        )
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("nl".parse::<LanguageFilter>()?)?)?
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(
        page_2.items(),
        nth(
            0,
            has_attribute(
                "street",
                attribute_of(AttributeKind::LanguageProperty)
                    .language("fr", "Grand Place")
                    .language("nl", "Grote Markt")
                    .without(Member::Lang)
            )
        )
    );
    Ok(())
}
