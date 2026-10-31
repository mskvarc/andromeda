//! A temporal values last n pairs only observed instances.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: speed holding 10, 20 and 30 observed hourly from 2024-01-01T00:00:00Z and 1000 and 2000 with no
/// `observedAt`, read in the simplified representation with `lastN` 2, answers values `[[20, 01:00]`, `[30,
/// 02:00]`]: each pair carries its `observedAt`
/// Clause: 4.5.9, 5.7.3.4, 6.3.12
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9013_a_temporal_values_last_n_pairs_only_observed_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1000)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-01T02:00:00Z".parse()?))
        .property("speed", 2000)
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&vehicle_id)
        .last_n(2)
        .send()
        .await?;
    let values = retrieved.body();
    expect_that!(values, has_values("speed", len(eq(2))));
    expect_that!(values, has_values("speed", nth(0, pair().value(20).time("2024-01-01T01:00:00Z".parse()?))));
    expect_that!(values, has_values("speed", nth(1, pair().value(30).time("2024-01-01T02:00:00Z".parse()?))));
    Ok(())
}
