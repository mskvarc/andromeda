//! Min of booleans answers a boolean.

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

/// Pins: min over the booleans true, false and true answers false
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: Table 4.5.19.1-1 orders booleans as 0 and 1 for min without saying whether the result is the number or
/// the boolean; Andromeda answers the boolean observed
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0017_min_of_booleans_answers_a_boolean() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("isActive", Property::builder(true).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("isActive", Property::builder(false).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("isActive", Property::builder(true).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("min".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated, has_aggregate("isActive", AggregationMethod::Min, nth(0, period().value(false))));
    Ok(())
}
