//! A month grid from the 28th pages by the full page size.

use andromeda_test_suite::{
    aggregated_answer::read,
    aggregation_matcher::{has_aggregate, period, successive_periods},
    broker,
    followed_link,
    header_matcher::absent,
    link_header::link_text,
    namespace::Namespace,
    nth::nth,
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

/// Pins: 24 instances, one per P1M period laid from 2020-07-28, answer a 206 of the full 19 periods ending on
/// 2022-02-28, whose rel="next" Link names `aggrPeriodOrigin` 2020-07-28 and answers the other 5; a request built
/// from the Content-Range end answers the same 5
/// Clause: 4.5.19, 4.5.19.1, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 periods per attribute (clause
/// 6.3.10 leaves the limit to the implementation)
/// Recorded: clause 6.3.10 defines no request that continues a partial response, and clause 4.5.19 does not say where
/// periods start; Andromeda's rel="next" Link names the origin of a calendar grid in `aggrPeriodOrigin` so the next
/// page keeps the grid of the first
/// Recorded: a grid laid from the 28th or earlier clamps no month, so every boundary lays the same grid and the page
/// keeps its full size
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9043_a_month_grid_from_the_28th_pages_by_the_full_page_size() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let grid_evolution = series(
        &namespace,
        &[
            "2020-07-29T00:00:00Z",
            "2020-08-29T00:00:00Z",
            "2020-09-29T00:00:00Z",
            "2020-10-29T00:00:00Z",
            "2020-11-29T00:00:00Z",
            "2020-12-29T00:00:00Z",
            "2021-01-29T00:00:00Z",
            "2021-03-01T00:00:00Z",
            "2021-03-29T00:00:00Z",
            "2021-04-29T00:00:00Z",
            "2021-05-29T00:00:00Z",
            "2021-06-29T00:00:00Z",
            "2021-07-29T00:00:00Z",
            "2021-08-29T00:00:00Z",
            "2021-09-29T00:00:00Z",
            "2021-10-29T00:00:00Z",
            "2021-11-29T00:00:00Z",
            "2021-12-29T00:00:00Z",
            "2022-01-29T00:00:00Z",
            "2022-03-01T00:00:00Z",
            "2022-03-29T00:00:00Z",
            "2022-04-29T00:00:00Z",
            "2022-05-29T00:00:00Z",
            "2022-06-29T00:00:00Z",
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
            temporal_query::between("2020-07-28T00:00:00Z".parse()?, "2022-07-28T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("P1M".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(has_range(
            "2020-07-28T00:00:00Z".parse()?,
            "2022-02-28T00:00:00Z".parse()?,
            ContentRangeSize::Unknown
        ))
    );
    let links = link_text(headed.head.headers());
    expect_that!(&links, contains_substring("timeAt=2022-02-28T00%3A00%3A00Z"));
    expect_that!(&links, contains_substring("aggrPeriodOrigin=2020-07-28T00%3A00%3A00Z"));
    expect_that!(
        headed.outcome.body(),
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                len(eq(19)),
                nth(0, period().value(0).start("2020-07-28T00:00:00Z".parse()?).end("2020-08-28T00:00:00Z".parse()?)),
                nth(
                    18,
                    period().value(18).start("2022-01-28T00:00:00Z".parse()?).end("2022-02-28T00:00:00Z".parse()?)
                )
            )
        )
    );
    let link = headed.head.typed::<Link>().into_valid().or_fail()?;
    let request = followed_link::request(&broker, &link, &RelationType::Next)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let page_2 = broker.send_unchecked(request).await?;
    expect_that!(page_2.status(), eq(StatusCode::OK));
    expect_that!(link_text(page_2.headers()), not(contains_substring("rel=\"next\"")));
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
                    "2022-02-28T00:00:00Z",
                    "2022-03-28T00:00:00Z",
                    "2022-04-28T00:00:00Z",
                    "2022-05-28T00:00:00Z",
                    "2022-06-28T00:00:00Z",
                    "2022-07-28T00:00:00Z",
                ]
            )?
        )
    );
    let headed_3 = json
        .with_heads()
        .temporal()
        .retrieve_aggregated(&grid_id)
        .temporal_q(
            temporal_query::between("2022-02-28T00:00:00Z".parse()?, "2022-07-28T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("P1M".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed_3.status, eq(StatusCode::OK));
    expect_that!(headed_3.head.typed::<ContentRange>(), absent());
    expect_that!(
        headed_3.outcome.body(),
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            successive_periods(
                19,
                &[
                    "2022-02-28T00:00:00Z",
                    "2022-03-28T00:00:00Z",
                    "2022-04-28T00:00:00Z",
                    "2022-05-28T00:00:00Z",
                    "2022-06-28T00:00:00Z",
                    "2022-07-28T00:00:00Z",
                ]
            )?
        )
    );
    Ok(())
}
