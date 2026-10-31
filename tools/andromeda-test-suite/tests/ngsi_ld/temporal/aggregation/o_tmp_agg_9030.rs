//! An aggregated last n above the maximum folds no instance without observed at.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    header_matcher::absent,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentRange},
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
};

/// Pins: speed holding 10, 20 and 30 observed hourly from 2024-01-01T00:00:00Z and 1000 and 2000 with no
/// `observedAt`, aggregated with `lastN` 5000, above the broker's maximum, answers 200 with no Content-Range: over
/// the whole range one period counting 3 whose sum is 60, and per PT1H three periods counting 1 each
/// Clause: 4.5.19, 5.7.3.4, 6.3.10
/// Recorded: Andromeda reading of v1.9.1: the aggregated representation folds the last n instances, which clause
/// 5.7.3.4 makes the last n timestamps of the temporal property; the base stack answers at most 19 periods per
/// attribute with `lastN` (clause 6.3.10 leaves the limit to the implementation)
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9030_an_aggregated_last_n_above_the_maximum_folds_no_instance_without_observed_at() -> Result<()> {
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
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .last_n(5000)
        .aggregation_methods("totalCount,sum".parse()?)
        .send()
        .await?;
    expect_that!(headed.head.typed::<ContentRange>(), absent());
    let retrieved = &headed.outcome;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate("speed", AggregationMethod::TotalCount, all!(len(eq(1)), nth(0, period().value(3))))
    );
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(60))));
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .last_n(5000)
        .aggregation_methods("totalCount,sum".parse()?)
        .aggregation_period("PT1H".parse()?)
        .send()
        .await?;
    expect_that!(headed_2.head.typed::<ContentRange>(), absent());
    let retrieved_2 = &headed_2.outcome;
    let aggregated_2 = retrieved_2.body();
    expect_that!(
        aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(len(eq(3)), nth(0, period().value(1)), nth(1, period().value(1)), nth(2, period().value(1)))
        )
    );
    expect_that!(
        aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(nth(0, period().value(10)), nth(1, period().value(20)), nth(2, period().value(30)))
        )
    );
    Ok(())
}
