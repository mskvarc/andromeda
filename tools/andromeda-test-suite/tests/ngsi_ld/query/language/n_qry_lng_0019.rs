//! Query simplified with a language answers the bare string.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, SimplifiedEntity},
    query::language_filter::LanguageFilter,
};

/// Pins: Query Entities with lang=nl and `options=keyValues` answers each `LanguageProperty` as the bare Dutch
/// string
/// Clause: 4.15, 5.7.2.5, 4.5.4
#[gtest]
#[tokio::test]
async fn n_qry_lng_0019_query_simplified_with_a_language_answers_the_bare_string() -> Result<()> {
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .entity_type(&building_type)
        .ids(vec![building_id.clone()])
        .lang("nl".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(predicate(|item: &SimplifiedEntity| simplified_member_count(item) == 3)));
    expect_that!(page.items(), contains(has_value("street", "Grote Markt")));
    Ok(())
}
