//! A grid of a month and a day walks its next links on the grid of the first page.

use andromeda_test_suite::{
    aggregated_answer::read,
    aggregation_matcher::{has_aggregate, period, successive_periods},
    broker,
    followed_link,
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

/// Pins: 24 instances, one per P1M1D period laid from 2020-01-15, answer a 206 of 19 periods ending on 2021-09-03,
/// whose rel="next" Link names `aggrPeriodOrigin` 2020-01-15 and answers the other 5 periods on the grid laid from
/// 2020-01-15, where one laid from 2021-09-03 would start its second on 2021-10-04 rather than 2021-10-05
/// Clause: 4.5.19, 4.5.19.1, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 periods per attribute (clause
/// 6.3.10 leaves the limit to the implementation)
/// Recorded: clause 6.3.10 defines no request that continues a partial response, and clause 4.5.19 does not say where
/// periods start; Andromeda's rel="next" Link names the origin of a calendar grid in `aggrPeriodOrigin` so the next
/// page keeps the grid of the first
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9042_a_grid_of_a_month_and_a_day_walks_its_next_links_on_the_grid_of_the_first_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let grid_evolution = series(
        &namespace,
        &[
            "2020-01-16T00:00:00Z",
            "2020-02-17T00:00:00Z",
            "2020-03-18T00:00:00Z",
            "2020-04-19T00:00:00Z",
            "2020-05-20T00:00:00Z",
            "2020-06-21T00:00:00Z",
            "2020-07-22T00:00:00Z",
            "2020-08-23T00:00:00Z",
            "2020-09-24T00:00:00Z",
            "2020-10-25T00:00:00Z",
            "2020-11-26T00:00:00Z",
            "2020-12-27T00:00:00Z",
            "2021-01-28T00:00:00Z",
            "2021-03-01T00:00:00Z",
            "2021-03-30T00:00:00Z",
            "2021-05-01T00:00:00Z",
            "2021-06-01T00:00:00Z",
            "2021-07-03T00:00:00Z",
            "2021-08-03T00:00:00Z",
            "2021-09-04T00:00:00Z",
            "2021-10-06T00:00:00Z",
            "2021-11-06T00:00:00Z",
            "2021-12-08T00:00:00Z",
            "2022-01-08T00:00:00Z",
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
            temporal_query::between("2020-01-15T00:00:00Z".parse()?, "2022-02-08T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("P1M1D".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(has_range(
            "2020-01-15T00:00:00Z".parse()?,
            "2021-09-03T00:00:00Z".parse()?,
            ContentRangeSize::Unknown
        ))
    );
    let links = link_text(headed.head.headers());
    expect_that!(&links, contains_substring("timeAt=2021-09-03T00%3A00%3A00Z"));
    expect_that!(&links, contains_substring("aggrPeriodOrigin=2020-01-15T00%3A00%3A00Z"));
    expect_that!(
        headed.outcome.body(),
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                len(eq(19)),
                nth(0, period().value(0).start("2020-01-15T00:00:00Z".parse()?).end("2020-02-16T00:00:00Z".parse()?)),
                nth(
                    18,
                    period().value(18).start("2021-08-02T00:00:00Z".parse()?).end("2021-09-03T00:00:00Z".parse()?)
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
                    "2021-09-03T00:00:00Z",
                    "2021-10-05T00:00:00Z",
                    "2021-11-05T00:00:00Z",
                    "2021-12-07T00:00:00Z",
                    "2022-01-07T00:00:00Z",
                    "2022-02-08T00:00:00Z",
                ]
            )?
        )
    );
    Ok(())
}
