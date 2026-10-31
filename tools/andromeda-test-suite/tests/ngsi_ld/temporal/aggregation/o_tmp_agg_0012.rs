//! Stddev of numbers is the sample deviation.

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

/// Pins: stddev over the numbers 100, 200 and 300 answers 100, their sample standard deviation
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: 4.5.19.1 asks for "the standard deviation" without choosing the sample or the population form;
/// Andromeda answers the sample standard deviation
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0012_stddev_of_numbers_is_the_sample_deviation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(200).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(300).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("stddev".parse()?)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_aggregate("speed", AggregationMethod::Stddev, nth(0, period().value_between(99.99, 100.01)))
    );
    Ok(())
}
