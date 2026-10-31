//! One period over a bounded query carries the query bounds.

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
    query::temporal_query,
};

/// Pins: avg over a between query from 00:00 to 03:00, with no `aggrPeriodDuration` and with PT0S, answers one
/// triple whose start and end `DateTimes` are the bounds of the query
/// Clause: 4.5.19, 4.5.19.1, 6.3.12, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0046_one_period_over_a_bounded_query_carries_the_query_bounds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:10:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T02:10:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-01T03:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Avg,
            all!(
                len(eq(1)),
                nth(
                    0,
                    period().value(15).start("2024-01-01T00:00:00Z".parse()?).end("2024-01-01T03:00:00Z".parse()?)
                )
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-01T03:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Avg])
                .aggregation_period("PT0S".parse()?),
        )
        .send()
        .await?;
    let aggregated_2 = retrieved_2.body();
    expect_that!(
        aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::Avg,
            all!(
                len(eq(1)),
                nth(
                    0,
                    period().value(15).start("2024-01-01T00:00:00Z".parse()?).end("2024-01-01T03:00:00Z".parse()?)
                )
            )
        )
    );
    Ok(())
}
