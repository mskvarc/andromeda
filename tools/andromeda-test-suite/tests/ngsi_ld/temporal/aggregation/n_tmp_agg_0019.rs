//! Total count of booleans.

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

/// Pins: `totalCount` over three boolean instances answers 3
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0019_total_count_of_booleans() -> Result<()> {
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
        .aggregation_methods("totalCount".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated, has_aggregate("isActive", AggregationMethod::TotalCount, nth(0, period().value(3))));
    Ok(())
}
