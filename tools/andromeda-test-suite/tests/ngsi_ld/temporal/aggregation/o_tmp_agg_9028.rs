//! A period grid places no instance without observed at.

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
/// `observedAt`, aggregated per PT1H with no temporal query and no `lastN`, answers three periods from 00:00, 01:00
/// and 02:00 each counting 1, with sums 10, 20 and 30
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: Andromeda reading of v1.9.1: an instance with no `observedAt` lies in no period of the grid (clause
/// 4.5.19); with no temporal query the grid starts at the least recent instant of the Entity's history
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9028_a_period_grid_places_no_instance_without_observed_at() -> Result<()> {
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
        .aggregation_period("PT1H".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(3)),
                nth(0, period().value(1).start("2024-01-01T00:00:00Z".parse()?).end("2024-01-01T01:00:00Z".parse()?)),
                nth(1, period().value(1).start("2024-01-01T01:00:00Z".parse()?)),
                nth(2, period().value(1).start("2024-01-01T02:00:00Z".parse()?))
            )
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(len(eq(3)), nth(0, period().value(10)), nth(1, period().value(20)), nth(2, period().value(30)))
        )
    );
    Ok(())
}
