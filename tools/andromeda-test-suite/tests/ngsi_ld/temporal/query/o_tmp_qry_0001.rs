//! Q ordering compares a language string by character code.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: a temporal q ordering or range on a `LanguageProperty` compares the string of the named language with the
/// query value, character by character
/// Clause: 4.9, 5.7.4.4, 4.5.18
/// Recorded: clause 4.9 makes the target value of a `LanguageProperty` the string of the named language (or any of
/// its strings) for every operator, but does not say how two strings are ordered; Andromeda compares them by
/// character code
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0001_q_ordering_compares_a_language_string_by_character_code() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_door_id = namespace.urn("Building", "door")?;
    let building_type = namespace.entity_type("Building")?;
    let building_gate_id = namespace.urn("Building", "gate")?;
    let building_door_evolution = TemporalEntity::builder()
        .id(building_door_id.clone())
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .q(q::attr("label").member("en").gt("E"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_gate_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .q(q::attr("label").member("en").within("C", "E"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_door_id)));
    Ok(())
}
