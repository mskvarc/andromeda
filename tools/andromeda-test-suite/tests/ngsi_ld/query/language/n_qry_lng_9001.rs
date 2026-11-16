//! A posted query with a ranked lang answers as the get query does.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Query Entities by POST of an Entity whose label `LanguageProperty` holds de, en and fr with `lang`
/// `fr-CH,en;q=0.9,*;q=0.5` answers the label as a Property holding the en string with lang en, the same Entity the
/// GET binding answers for that filter
/// Clause: 4.15, 6.23.1, Table 5.2.23-1
/// Recorded: Table 5.2.23-1 gives the posted Query the `lang` member of the `lang` query parameter, so the ranked
/// filter of clause 4.15 example 4 applies to both bindings alike
#[gtest]
#[tokio::test]
async fn n_qry_lng_9001_a_posted_query_with_a_ranked_lang_answers_as_the_get_query_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("de", "rot"), ("en", "red"), ("fr", "rouge")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let lang: LanguageFilter = "fr-CH,en;q=0.9,*;q=0.5".parse()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let posted = json
        .entities()
        .query()
        .entity_type(&building_type)
        .lang(lang.clone())
        .method(QueryMethod::Post)
        .send()
        .await?;
    expect_that!(posted.items(), len(eq(1)));
    expect_that!(
        posted.items(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_attribute("label", property().value("red").lang("en").without(Member::LanguageMap))
            )
        )
    );
    let by_get = json.entities().query().entity_type(&building_type).lang(lang).send().await?;
    expect_that!(by_get.items(), eq(posted.items()));
    Ok(())
}
