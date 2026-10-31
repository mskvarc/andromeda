//! A post query negated pattern with a language matches an entity without that language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by POST with q name`[en]`!~="^D" answers the entity whose en string fails the pattern and
/// the entity that holds no en entry, and not the entity whose en string matches it
/// Clause: 4.9, 5.2.23, 6.23.3.1
/// Recorded: clause 4.9 says a matching entity for `notPatternOp` shall contain the target element and the target value
/// shall not be in the L(R); the entity whose `LanguageProperty` holds no entry for the language contains the target
/// element and has no value for that key, so the target value is not in L(R), the way unequal matches it; the clause
/// does not spell it out for `notPatternOp`
#[gtest]
#[tokio::test]
async fn o_qry_qlg_4017_a_post_query_negated_pattern_with_a_language_matches_an_entity_without_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_door_id = namespace.urn("Building", "door")?;
    let building_type = namespace.entity_type("Building")?;
    let building_gate_id = namespace.urn("Building", "gate")?;
    let building_porte_id = namespace.urn("Building", "porte")?;
    let entities = [
        Entity::builder()
            .id(building_door_id)
            .entity_type(building_type.clone())
            .language_property(
                "name",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Door"), ("de", "Zimmer")])?),
            )
            .build()?,
        Entity::builder()
            .id(building_gate_id.clone())
            .entity_type(building_type.clone())
            .language_property(
                "name",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Gate"), ("de", "Ausgang")])?),
            )
            .build()?,
        Entity::builder()
            .id(building_porte_id.clone())
            .entity_type(building_type.clone())
            .language_property("name", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Porte")])?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .q(q::attr("name").member("en").not_matches("\"^D\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_gate_id)));
    expect_that!(page.items(), contains(has_id(&building_porte_id)));
    Ok(())
}
