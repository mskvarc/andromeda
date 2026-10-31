//! A whole range aggregation folds no instance without observed at.

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
/// `observedAt`, aggregated over the whole range with no temporal query and no `lastN`, answers one period from
/// 2024-01-01T00:00:00Z to 02:00 counting 3 whose sum is 60
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: Andromeda reading of v1.9.1: clause 4.5.19 places every aggregate in a period of the time range, a zero
/// `aggrPeriodDuration` "spanning the whole time-range", and an instance with no `observedAt` lies in no period, the
/// whole range included; the open bounds are the least and most recent instants of the response
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9027_a_whole_range_aggregation_folds_no_instance_without_observed_at() -> Result<()> {
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
        .aggregation_methods("totalCount,sum".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(1)),
                nth(0, period().value(3).start("2024-01-01T00:00:00Z".parse()?).end("2024-01-01T02:00:00Z".parse()?))
            )
        )
    );
    expect_that!(
        aggregated,
        has_aggregate("speed", AggregationMethod::Sum, all!(len(eq(1)), nth(0, period().value(60))))
    );
    Ok(())
}
