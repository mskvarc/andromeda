//! A monthly grid from the 30th keeps its day, clamped in February, and refuses a mixed duration past its budget.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, successive_periods},
    broker,
    namespace::Namespace,
    outcome::Upserted,
    period_grid::series,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, evolution::AggregationMethod},
    query::temporal_query,
};

/// Pins: a monthly grid anchored on 30 January 2024 keeps the 30th, so its boundary in February is the 29th of the
/// leap year, and 30 April is an ordinary boundary of it. A duration mixing months with days has no legal cut, so a
/// read of it over the page budget is refused with `TooManyResults`
/// Clause: 4.5.19, 4.5.19.1, 6.3.20
/// Recorded: Andromeda's calendar grid rule and its mixed-duration refusal, on the base stack's period budget of 19
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9040_a_monthly_grid_from_the_30th_keeps_its_day_and_a_mixed_duration_past_the_budget_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let grid_evolution = series(
        &namespace,
        &[
            "2024-02-01T00:00:00Z",
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
            "2025-03-01T00:00:00Z",
            "2025-04-01T00:00:00Z",
            "2025-05-01T00:00:00Z",
            "2025-06-01T00:00:00Z",
            "2025-07-01T00:00:00Z",
            "2025-08-01T00:00:00Z",
            "2025-09-01T00:00:00Z",
            "2025-10-01T00:00:00Z",
            "2025-11-01T00:00:00Z",
            "2025-12-01T00:00:00Z",
            "2026-01-01T00:00:00Z",
            "2026-02-01T00:00:00Z",
            "2026-03-01T00:00:00Z",
        ],
    )?;
    let grid_id = grid_evolution.id.clone();
    broker.temporal().create_or_update(&grid_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let headed = json
        .with_heads()
        .temporal()
        .retrieve_aggregated(&grid_id)
        .temporal_q(
            temporal_query::between("2024-01-30T00:00:00Z".parse()?, "2024-07-30T00:00:00Z".parse()?)
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
                    "2024-01-30T00:00:00Z",
                    "2024-02-29T00:00:00Z",
                    "2024-03-30T00:00:00Z",
                    "2024-04-30T00:00:00Z",
                    "2024-05-30T00:00:00Z",
                    "2024-06-30T00:00:00Z",
                    "2024-07-30T00:00:00Z",
                ]
            )?
        )
    );
    let answer = request_to(
        &json,
        Method::GET,
        &format!(
            "temporal/entities/{grid_id}?timerel=between&timeAt=2024-01-30T00%3A00%3A00Z&endTimeAt=2026-03-30T00%3A00%3A00Z&format=aggregatedValues&aggrMethods=sum&aggrPeriodDuration=P1M1D"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let refused = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(answer).await;
    expect_that!(&refused, problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults));
    Ok(())
}
