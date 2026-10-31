//! A before request continues as a between window closing where it closed.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    followed_link,
    link_header::link_text,
    namespace::Namespace,
    outcome::Upserted,
    paged_history::{has_range, read, series, stamp},
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    StatusCode,
    header::{ACCEPT, CONTENT_RANGE, LINK},
};
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize, Link, RelationType},
    model::NgsiLdMediaType,
    query::temporal_query,
};

/// Pins: A Temporal Evolution of 25 speed instances read forwards with timerel before 00:30 answers a 206 of the first
/// 19 whose rel="next" Link asks for timerel between from one microsecond past 00:18 to endTimeAt 00:30, which answers a
/// 200 of the other 6
/// Clause: 4.11, 5.7.3.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// lastN and with lastN (clause 6.3.10 leaves both to the implementation)
/// Recorded: clause 6.3.10 defines no request that continues a partial response; Andromeda points a partial retrieve at
/// one with a rel="next" Link typed as the response, which opens one microsecond past a forward page's last instant and
/// asks a backward page's continuation for the client's lastN less the fewest instances any attribute received
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9067_a_before_request_continues_as_a_between_window_closing_where_it_closed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let speed = (0..25_i32).map(|value| (value, value.unsigned_abs())).collect::<Vec<_>>();
    let paged_series_before_evolution = series(&namespace, "before", &speed, &[])?;
    let paged_series_before_id = paged_series_before_evolution.id.clone();
    broker.temporal().create_or_update(&paged_series_before_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .retrieve(&paged_series_before_id)
        .temporal_q(temporal_query::before("2024-01-01T00:30:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(has_range(stamp(0)?, stamp(18)?, ContentRangeSize::Unknown))
    );
    let links = link_text(headed.head.headers());
    expect_that!(&links, contains_substring("timerel=between"));
    expect_that!(&links, contains_substring("timeAt=2024-01-01T00%3A18%3A00.000001Z"));
    expect_that!(&links, contains_substring("endTimeAt=2024-01-01T00%3A30%3A00Z"));
    let evolution = headed.outcome.body();
    expect_that!(evolution, has_evolution("speed", len(eq(19))));
    for value in 0..=18 {
        expect_that!(evolution, has_evolution("speed", contains(any_attribute().value(value))));
    }
    let link = headed.head.typed::<Link>().into_valid().or_fail()?;
    let request = followed_link::request(&broker, &link, &RelationType::Next)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let page_2 = broker.send_unchecked(request).await?;
    expect_that!(page_2.status(), eq(StatusCode::OK));
    expect_that!(page_2.headers().get(CONTENT_RANGE), none());
    expect_that!(page_2.headers().get(LINK), none());
    let evolution_2 = read(&page_2)?;
    expect_that!(&evolution_2, has_evolution("speed", len(eq(6))));
    for value in 19..=24 {
        expect_that!(&evolution_2, has_evolution("speed", contains(any_attribute().value(value))));
    }
    Ok(())
}
