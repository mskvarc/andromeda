//! A monthly grid from the last day of January pages on a month end, and a request built from that boundary repeats the next page.

use andromeda_test_suite::{
    aggregated_answer::read,
    aggregation_matcher::{has_aggregate, successive_periods},
    broker,
    followed_link,
    link_header::link_text,
    namespace::Namespace,
    outcome::Upserted,
    paged_history::has_range,
    period_grid::series,
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    StatusCode,
    header::{ACCEPT, CONTENT_RANGE},
};
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize, Link, RelationType},
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::temporal_query,
};

/// Pins: every boundary of a monthly grid from 31 January 2024 is a month end, so a page of 19 periods ends on 31
/// August 2025 and its next link continues on that boundary; the second page keeps the month ends of the grid
/// Clause: 4.5.19, 6.3.10
/// Recorded: Andromeda's calendar grid rule; the base stack answers at most 19 periods per attribute
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9042_a_monthly_grid_from_the_last_day_of_january_pages_on_a_month_end() -> Result<()> {
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
            temporal_query::between("2024-01-31T00:00:00Z".parse()?, "2026-03-31T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("P1M".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(has_range(
            "2024-01-31T00:00:00Z".parse()?,
            "2025-08-31T00:00:00Z".parse()?,
            ContentRangeSize::Unknown
        ))
    );
    let links = link_text(headed.head.headers());
    expect_that!(&links, contains_substring("timeAt=2025-08-31T00%3A00%3A00Z"));
    expect_that!(
        headed.outcome.body(),
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            successive_periods(
                0,
                &[
                    "2024-01-31T00:00:00Z",
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
                    "2025-03-31T00:00:00Z",
                    "2025-04-30T00:00:00Z",
                    "2025-05-31T00:00:00Z",
                    "2025-06-30T00:00:00Z",
                    "2025-07-31T00:00:00Z",
                    "2025-08-31T00:00:00Z",
                ]
            )?
        )
    );
    let link = headed.head.typed::<Link>().into_valid().or_fail()?;
    let request = followed_link::request(&broker, &link, &RelationType::Next)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let page_2 = broker.send_unchecked(request).await?;
    expect_that!(page_2.status(), eq(StatusCode::OK));
    expect_that!(page_2.headers().get(CONTENT_RANGE), none());
    let aggregated_2 = read(&page_2)?;
    expect_that!(
        &aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            successive_periods(
                19,
                &[
                    "2025-08-31T00:00:00Z",
                    "2025-09-30T00:00:00Z",
                    "2025-10-31T00:00:00Z",
                    "2025-11-30T00:00:00Z",
                    "2025-12-31T00:00:00Z",
                    "2026-01-31T00:00:00Z",
                    "2026-02-28T00:00:00Z",
                    "2026-03-31T00:00:00Z",
                ]
            )?
        )
    );
    Ok(())
}
