//! A yearly grid from 29 February keeps the last day of February, and an undefined `aggrPeriodOrigin` is refused.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, successive_periods},
    broker,
    namespace::Namespace,
    outcome::Upserted,
    period_grid::series,
    problem::answered,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::temporal_query,
};

/// Pins: a yearly grid anchored on 29 February 2024 has boundaries on 28 February in non-leap years and on 29 February
/// in leap years, and a request naming `aggrPeriodOrigin`, which the specification does not define, is answered 400
/// Clause: 4.5.19, 6.3.20
/// Recorded: Andromeda's calendar grid rule, which keeps every boundary on a month end when the anchor is one
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9043_a_yearly_grid_from_29_february_keeps_the_last_day_of_february_and_refuses_an_origin() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let grid_evolution = series(
        &namespace,
        &[
            "2025-02-01T00:00:00Z",
            "2026-02-01T00:00:00Z",
            "2027-02-01T00:00:00Z",
            "2028-02-01T00:00:00Z",
            "2029-02-01T00:00:00Z",
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
            temporal_query::between("2024-02-29T00:00:00Z".parse()?, "2029-02-28T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("P1Y".parse()?),
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
                    "2025-02-28T00:00:00Z",
                    "2026-02-28T00:00:00Z",
                    "2027-02-28T00:00:00Z",
                    "2028-02-29T00:00:00Z",
                    "2029-02-28T00:00:00Z",
                ]
            )?
        )
    );
    let answer = request_to(
        &json,
        Method::GET,
        &format!(
            "temporal/entities/{grid_id}?timerel=between&timeAt=2024-02-29T00%3A00%3A00Z&endTimeAt=2029-02-28T00%3A00%3A00Z&format=aggregatedValues&aggrMethods=sum&aggrPeriodDuration=P1Y&aggrPeriodOrigin=2024-02-29T00%3A00%3A00Z"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let refused = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(answer).await;
    expect_that!(&refused, answered(StatusCode::BAD_REQUEST));
    Ok(())
}
