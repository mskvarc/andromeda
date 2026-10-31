//! Max of strings is the last in lexicographic order.

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

/// Pins: max over the strings banana, apple and cherry answers cherry
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0022_max_of_strings_is_the_last_in_lexicographic_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("status", Property::builder("banana").observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("status", Property::builder("apple").observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("status", Property::builder("cherry").observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("max".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated, has_aggregate("status", AggregationMethod::Max, nth(0, period().value("cherry"))));
    Ok(())
}
