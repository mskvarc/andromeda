//! Observations spanning a year written at once are each selected by their own window.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Create Temporal Evolution of reading with four observations made a year, a month, a day and no time before
/// 2026-10-12 answers 201, and a two-minute window around each `observedAt` holds that observation alone
/// Clause: 4.11, 5.6.11, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9037_observations_spanning_a_year_written_at_once_are_each_selected_by_their_own_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_late_id = namespace.urn("Sensor", "late")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_late_evolution = TemporalEntity::builder()
        .id(sensor_late_id.clone())
        .entity_type(sensor_type)
        .property("reading", Property::builder(365).observed_at("2025-10-12T00:00:00.000Z".parse()?))
        .property("reading", Property::builder(30).observed_at("2026-09-12T00:00:00.000Z".parse()?))
        .property("reading", Property::builder(1).observed_at("2026-10-11T00:00:00.000Z".parse()?))
        .property("reading", Property::builder(0).observed_at("2026-10-12T00:00:00.000Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_late_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_late_id)
        .temporal_q(temporal_query::between("2025-10-11T23:59:00Z".parse()?, "2025-10-12T00:01:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("reading", all!(len(eq(1)), nth(0, any_attribute().value(365)))));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_late_id)
        .temporal_q(temporal_query::between("2026-09-11T23:59:00Z".parse()?, "2026-09-12T00:01:00Z".parse()?))
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("reading", all!(len(eq(1)), nth(0, any_attribute().value(30)))));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_late_id)
        .temporal_q(temporal_query::between("2026-10-10T23:59:00Z".parse()?, "2026-10-11T00:01:00Z".parse()?))
        .send()
        .await?;
    let evolution_3 = retrieved_3.body();
    expect_that!(evolution_3, has_evolution("reading", all!(len(eq(1)), nth(0, any_attribute().value(1)))));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&sensor_late_id)
        .temporal_q(temporal_query::between("2026-10-11T23:59:00Z".parse()?, "2026-10-12T00:01:00Z".parse()?))
        .send()
        .await?;
    let evolution_4 = retrieved_4.body();
    expect_that!(evolution_4, has_evolution("reading", all!(len(eq(1)), nth(0, any_attribute().value(0)))));
    Ok(())
}
