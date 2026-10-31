//! A temporal q negated pattern with a language matches an entity without that language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: a temporal q label`[en]`!~="^D" answers the entity whose en string fails the pattern and the entity whose
/// `LanguageProperty` holds no en entry, and not the entity whose en string matches it
/// Clause: 4.9, 5.7.4.4, 4.5.18
/// Recorded: clause 4.9 says a matching entity for `notPatternOp` shall contain the target element and the target value
/// shall not be in the L(R); the entity whose `LanguageProperty` holds no entry for the language contains the target
/// element and has no value for that key, so the target value is not in L(R), the way unequal matches it; the clause
/// does not spell it out for `notPatternOp`
#[gtest]
#[tokio::test]
async fn o_tmp_qry_4007_a_temporal_q_negated_pattern_with_a_language_matches_an_entity_without_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_door_id = namespace.urn("Building", "door")?;
    let building_type = namespace.entity_type("Building")?;
    let building_gate_id = namespace.urn("Building", "gate")?;
    let building_porte_id = namespace.urn("Building", "porte")?;
    let building_door_evolution = TemporalEntity::builder()
        .id(building_door_id)
        .entity_type(building_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Door"), ("de", "Tuer")])?).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_door_evolution).send().await?.created()?;
    let building_gate_evolution = TemporalEntity::builder()
        .id(building_gate_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Gate"), ("de", "Dach")])?).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_gate_evolution).send().await?.created()?;
    let building_porte_evolution = TemporalEntity::builder()
        .id(building_porte_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Porte")])?).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_porte_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .q(q::attr("label").member("en").not_matches("\"^D\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_gate_id)));
    expect_that!(page.items(), contains(has_id(&building_porte_id)));
    Ok(())
}
