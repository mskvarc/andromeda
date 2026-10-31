//! An aggregated last n folds the most recent observed instances only.

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

/// Pins: speed holding 10, 20 and 30 observed hourly from 2024-01-01T00:00:00Z and 1000 and 2000 with no
/// `observedAt`, aggregated with `lastN` 2 and no temporal query, answers over the whole range one period counting
/// 2 whose sum is 50, and per PT1H two periods from 01:00 and 02:00 with sums 20 and 30
/// Clause: 4.5.19, 5.7.3.4, Table 5.2.21-1
/// Recorded: Andromeda reading of v1.9.1: `lastN` keeps "the last n instances, per Attribute" whatever the
/// representation and the aggregated representation folds those; clause 5.7.3.4 makes them the last n timestamps of the
/// temporal property, which an instance with no `observedAt` does not carry
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9029_an_aggregated_last_n_folds_the_most_recent_observed_instances_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", 1000)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-01T02:00:00Z".parse()?))
        .property("speed", 2000)
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .last_n(2)
        .aggregation_methods("totalCount,sum".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate("speed", AggregationMethod::TotalCount, all!(len(eq(1)), nth(0, period().value(2))))
    );
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(50))));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .last_n(2)
        .aggregation_methods("totalCount,sum".parse()?)
        .aggregation_period("PT1H".parse()?)
        .send()
        .await?;
    let aggregated_2 = retrieved_2.body();
    expect_that!(
        aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(2)),
                nth(0, period().value(1).start("2024-01-01T01:00:00Z".parse()?)),
                nth(1, period().value(1).start("2024-01-01T02:00:00Z".parse()?))
            )
        )
    );
    expect_that!(
        aggregated_2,
        has_aggregate("speed", AggregationMethod::Sum, all!(nth(0, period().value(20)), nth(1, period().value(30))))
    );
    Ok(())
}
