//! A post query answers the accepted language.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    wire::vary,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::{
        headers::{Accept, AcceptLanguage},
        operation::QueryMethod,
    },
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Query Entities by POST with Accept-Language fr and a Query body without lang answers the street
/// `LanguageProperty` as a Property holding the French string with lang fr, and the response varies on
/// accept-language
/// Clause: 4.15, 5.7.2.4, 6.23.3.1
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0003_a_post_query_answers_the_accepted_language() -> Result<()> {
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
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("fr".parse::<LanguageFilter>()?)?)?
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(vary(headed.head.headers()), contains(eq(&HeaderName::from_static("accept-language"))));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_attribute("street", property().value("Grand Place").lang("fr").without(Member::LanguageMap))
            )
        )
    );
    Ok(())
}
