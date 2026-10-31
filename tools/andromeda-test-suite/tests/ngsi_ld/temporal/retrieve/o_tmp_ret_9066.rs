//! Following next links walks a forward history serving every instance once.

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
    http::{
        ResponseHead,
        headers::{Accept, ContentRange, ContentRangeSize, Link, RelationType},
    },
    model::{DateTime, NgsiLdMediaType, Output, evolution::TemporalEntity},
    query::temporal_query,
};
use std::ops::RangeInclusive;

/// Pins: A Temporal Evolution of 60 speed instances read forwards without lastN, whose pages of 19 end on groups of 3
/// instances sharing an instant, answers 206 pages of 21, 21 and then a 200 of 18 when each rel="next" Link is followed,
/// together holding every instance exactly once
/// Clause: 4.11, 5.7.3.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// lastN and with lastN (clause 6.3.10 leaves both to the implementation)
/// Recorded: clause 6.3.10 defines no request that continues a partial response; Andromeda points a partial retrieve at
/// one with a rel="next" Link typed as the response, which opens one microsecond past a forward page's last instant and
/// asks a backward page's continuation for the client's lastN less the fewest instances any attribute received
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9066_following_next_links_walks_a_forward_history_serving_every_instance_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let speed = (0..60_i32)
        .map(|index| {
            let minutes = match index {
                i32::MIN..=18 | 60..=i32::MAX => index,
                19 | 20 => 18,
                21..=39 => index - 2,
                40 | 41 => 37,
                42..=59 => index - 4,
            };
            (index, minutes.unsigned_abs())
        })
        .collect::<Vec<_>>();
    let paged_series_forward_evolution = series(&namespace, "forward", &speed, &[])?;
    let paged_series_forward_id = paged_series_forward_evolution.id.clone();
    broker.temporal().create_or_update(&paged_series_forward_evolution).send().await?.created()?;
    let holds = |evolution: &TemporalEntity<Output>, values: RangeInclusive<i32>| -> Result<()> {
        verify_that!(evolution, has_evolution("speed", len(eq(values.clone().count()))))?;
        for value in values {
            verify_that!(evolution, has_evolution("speed", contains(any_attribute().value(value))))?;
        }
        Ok(())
    };
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .retrieve(&paged_series_forward_id)
        .temporal_q(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(has_range(stamp(0)?, stamp(18)?, ContentRangeSize::Unknown))
    );
    let links = link_text(headed.head.headers());
    expect_that!(&links, contains_substring("timeAt=2024-01-01T00%3A18%3A00.000001Z"));
    holds(headed.outcome.body(), 0..=20)?;
    let link = headed.head.typed::<Link>().into_valid().or_fail()?;
    let request = followed_link::request(&broker, &link, &RelationType::Next)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let page_2 = broker.send_unchecked(request).await?;
    expect_that!(page_2.status(), eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        ResponseHead::new(page_2.headers().clone()).typed::<ContentRange>().valid(),
        some(has_range(
            "2024-01-01T00:18:00.000001Z".parse::<DateTime>()?,
            stamp(37)?,
            ContentRangeSize::Unknown
        ))
    );
    holds(&read(&page_2)?, 21..=41)?;
    let link_2 = ResponseHead::new(page_2.headers().clone()).typed::<Link>().into_valid().or_fail()?;
    let request_2 = followed_link::request(&broker, &link_2, &RelationType::Next)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let page_3 = broker.send_unchecked(request_2).await?;
    expect_that!(page_3.status(), eq(StatusCode::OK));
    expect_that!(page_3.headers().get(CONTENT_RANGE), none());
    expect_that!(page_3.headers().get(LINK), none());
    holds(&read(&page_3)?, 42..=59)?;
    Ok(())
}
