//! Q pattern reads a language property through its language map.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: a temporal q pattern on a `LanguageProperty` matches the string of the named language, or any of its
/// strings when no language is named
/// Clause: 4.9, 5.7.4.4, 4.5.18
#[gtest]
#[tokio::test]
async fn n_tmp_qry_0006_q_pattern_reads_a_language_property_through_its_language_map() -> Result<()> {
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
        .q(q::attr("label").member("en").matches("\"^D\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_door_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .q(q::attr("label").member("de").matches("\"^D\""))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_gate_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .q(q::attr("label").matches("\"^Tu\""))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_door_id)));
    Ok(())
}
