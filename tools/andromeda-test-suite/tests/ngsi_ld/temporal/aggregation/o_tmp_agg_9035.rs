//! Following the next link walks aggregated periods on their boundary.

use andromeda_test_suite::{
    aggregated_answer::read,
    aggregation_matcher::{has_aggregate, period},
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
    header::{ACCEPT, CONTENT_RANGE, LINK},
};
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize, Link, RelationType},
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::temporal_query,
};

/// Pins: 30 hourly instances aggregated per PT1H after 2024-01-01 answer a 206 of the first 19 periods whose
/// rel="next" Link opens on 19:00, the end of the last period served, which answers a 200 of the other 11, the
/// periods of both pages contiguous and each counting one instance
/// Clause: 4.5.19, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: a period is the element of the aggregated representation, and the
/// base stack answers at most 19 of them per attribute (clause 6.3.10 leaves the limit to the implementation); a
/// page ends at the end of the last period it serves
/// Recorded: clause 6.3.10 defines no request that continues a partial response; Andromeda points a partial
/// retrieve at one with a rel="next" Link typed as the response, which opens one microsecond past a forward page's
/// last instant and asks a backward page's continuation for the client's `lastN` less the fewest instances any
/// attribute received
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9035_following_the_next_link_walks_aggregated_periods_on_their_boundary() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let hours = (0..30)
        .map(|hour| format!("2024-01-0{}T{:02}:00:00Z", 1 + hour / 24, hour % 24))
        .collect::<Vec<_>>();
    let instants = hours.iter().map(String::as_str).collect::<Vec<_>>();
    let walk_evolution = series(&namespace, &instants)?;
    let walk_id = walk_evolution.id.clone();
    broker.temporal().create_or_update(&walk_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .retrieve_aggregated(&walk_id)
        .temporal_q(
            temporal_query::after("2024-01-01T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount])
                .aggregation_period("PT1H".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(has_range(
            "2024-01-01T00:00:00Z".parse()?,
            "2024-01-01T19:00:00Z".parse()?,
            ContentRangeSize::Unknown
        ))
    );
    expect_that!(link_text(headed.head.headers()), contains_substring("timeAt=2024-01-01T19%3A00%3A00Z"));
    expect_that!(
        headed.outcome.body(),
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(19)),
                nth(0, period().value(1).start("2024-01-01T00:00:00Z".parse()?)),
                nth(18, period().value(1).end("2024-01-01T19:00:00Z".parse()?))
            )
        )
    );
    let link = headed.head.typed::<Link>().into_valid().or_fail()?;
    let request = followed_link::request(&broker, &link, &RelationType::Next)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let page_2 = broker.send_unchecked(request).await?;
    expect_that!(page_2.status(), eq(StatusCode::OK));
    expect_that!(page_2.headers().get(CONTENT_RANGE), none());
    expect_that!(page_2.headers().get(LINK), none());
    let aggregated_2 = read(&page_2)?;
    expect_that!(
        &aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(11)),
                nth(0, period().value(1).start("2024-01-01T19:00:00Z".parse()?)),
                nth(10, period().value(1).end("2024-01-02T06:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
