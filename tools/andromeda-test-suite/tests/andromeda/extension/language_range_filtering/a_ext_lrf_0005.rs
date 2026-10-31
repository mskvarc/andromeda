//! A temporal query with a bare language range matches a regional variant.

use andromeda_test_suite::{broker, config::BrokerRole, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: at the language-ranges level, Query Temporal Evolution of Entities with q label`[en]`=="Lift" answers the
/// entity whose label instance holds the operand under en-GB and not the entity whose instance holds it under fr
/// Clause: 4.9, 5.7.4.4
/// Recorded: Andromeda extension, not v1.9.1: the language-ranges level of the `q_target_languages` extension reads
/// a q language selector as an RFC 4647 extended range matched by section 3.3.2 extended filtering, where clause
/// 4.9 compares a target language as one tag
#[gtest]
#[tokio::test]
async fn a_ext_lrf_0005_a_temporal_query_with_a_bare_language_range_matches_a_regional_variant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_language_range = broker::client(BrokerRole::LanguageRange)?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker_language_range.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    let sensor_2_evolution = TemporalEntity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Lift")])?).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker_language_range.temporal().create_or_update(&sensor_2_evolution).send().await?.created()?;
    let page = broker_language_range
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .q(q::attr("label").member("en").eq("Lift"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
