//! A monthly grid from the last day of February keeps every boundary on a month end.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, successive_periods},
    broker,
    namespace::Namespace,
    outcome::Upserted,
    period_grid::series,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::temporal_query,
};

/// Pins: a monthly grid anchored on 29 February 2024 puts its boundaries on the last day of each month (31 March, 30
/// April, 31 May and so on), so the twelve periods of the window end on 31 January and 28 February 2025
/// Clause: 4.5.19, 4.5.19.1
/// Recorded: Andromeda's calendar grid rule, which keeps every boundary on a month end when the anchor is one
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9039_a_monthly_grid_from_the_last_day_of_february_keeps_every_boundary_on_a_month_end() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let grid_evolution = series(
        &namespace,
        &[
            "2024-03-01T00:00:00Z",
            "2024-04-01T00:00:00Z",
            "2024-05-01T00:00:00Z",
            "2024-06-01T00:00:00Z",
            "2024-07-01T00:00:00Z",
            "2024-08-01T00:00:00Z",
            "2024-09-01T00:00:00Z",
            "2024-10-01T00:00:00Z",
            "2024-11-01T00:00:00Z",
            "2024-12-01T00:00:00Z",
            "2025-01-01T00:00:00Z",
            "2025-02-01T00:00:00Z",
        ],
    )?;
    let grid_id = grid_evolution.id.clone();
    broker.temporal().create_or_update(&grid_evolution).send().await?.created()?;
    let headed = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_heads()
        .temporal()
        .retrieve_aggregated(&grid_id)
        .temporal_q(
            temporal_query::between("2024-02-29T00:00:00Z".parse()?, "2025-02-28T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("P1M".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(
        headed.outcome.body(),
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            successive_periods(
                0,
                &[
                    "2024-02-29T00:00:00Z",
                    "2024-03-31T00:00:00Z",
                    "2024-04-30T00:00:00Z",
                    "2024-05-31T00:00:00Z",
                    "2024-06-30T00:00:00Z",
                    "2024-07-31T00:00:00Z",
                    "2024-08-31T00:00:00Z",
                    "2024-09-30T00:00:00Z",
                    "2024-10-31T00:00:00Z",
                    "2024-11-30T00:00:00Z",
                    "2024-12-31T00:00:00Z",
                    "2025-01-31T00:00:00Z",
                    "2025-02-28T00:00:00Z",
                ]
            )?
        )
    );
    Ok(())
}
