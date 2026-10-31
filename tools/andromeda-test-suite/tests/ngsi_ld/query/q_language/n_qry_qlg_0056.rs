//! A language selector matches a map key in either case.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with q label`[en]`=="Lift" and with q label`[EN]`=="Lift" each answers the entities whose
/// `languageMap` holds the operand under the key en and under the key EN, and not the entity holding it under fr
/// Clause: 4.9, 4.5.18
/// Recorded: clause 4.5.18.2 keys a `languageMap` by RFC 5646 tags, and RFC 5646 section 2.1.1 makes a tag carry no
/// distinction of case
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0056_a_language_selector_matches_a_map_key_in_either_case() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_3_id = namespace.urn("Sensor", 3)?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Lift")])?))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id.clone())
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("EN", "Lift")])?))
        .build()?;
    broker.entities().create(&sensor_2).send().await?.done()?;
    let sensor_3 = Entity::builder()
        .id(sensor_3_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Lift")])?))
        .build()?;
    broker.entities().create(&sensor_3).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en").eq("Lift"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&sensor_id)));
    expect_that!(page.items(), contains(has_id(&sensor_2_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("label").member("EN").eq("Lift"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&sensor_id)));
    expect_that!(page_2.items(), contains(has_id(&sensor_2_id)));
    Ok(())
}
