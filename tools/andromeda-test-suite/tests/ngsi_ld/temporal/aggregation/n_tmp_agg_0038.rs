//! Each attribute is aggregated on its own.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
};

/// Pins: avg over an Entity with speed 100 and 200 and fuel 50 and 30 answers 150 for speed and 40 for fuel
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0038_each_attribute_is_aggregated_on_its_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(200).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("fuel", Property::builder(50).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("fuel", Property::builder(30).observed_at("2024-01-01T01:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("avg".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Avg, nth(0, period().value(150))));
    expect_that!(aggregated, has_aggregate("fuel", AggregationMethod::Avg, nth(0, period().value(40))));
    Ok(())
}
