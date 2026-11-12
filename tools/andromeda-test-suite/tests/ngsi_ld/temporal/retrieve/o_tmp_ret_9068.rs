//! Following next links walks a backward last n serving every instance once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    followed_link,
    link_header::link_text,
    namespace::Namespace,
    outcome::Upserted,
    paged_history::{has_range, read, series, stamp},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Response,
    StatusCode,
    header::{ACCEPT, CONTENT_RANGE, LINK},
};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Accept, ContentRange, ContentRangeSize, Link, RelationType},
        operation::{NormalizedTemporal, RetrieveTemporalEvolution, TemporalSelection},
    },
    model::{NgsiLdMediaType, Output, evolution::TemporalEntity},
};
use std::{num::NonZeroU64, ops::RangeInclusive};

/// Pins: A Temporal Evolution of 100 speed instances, one a minute, and 30 sparser fuel instances, one every two
/// minutes, read with lastN 50 answers 206 pages backwards whose rel="next" Links ask for lastN 50, 50, 42, 32 and 23 as
/// fuel receives 0, 0, 8, 10 and 9 instances, and then a 200, together holding every instance exactly once
/// Clause: 4.11, 5.7.3.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// lastN and with lastN (clause 6.3.10 leaves both to the implementation)
/// Recorded: clause 6.3.10 defines no request that continues a partial response; Andromeda points a partial retrieve at
/// one with a rel="next" Link typed as the response, which opens one microsecond past a forward page's last instant and
/// asks a backward page's continuation for the client's lastN less the fewest instances any attribute received
/// Recorded: a backward walk serves an attribute past its lastN by what it receives beyond the sparsest attribute on
/// each page; here speed receives all 100 instances
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9068_following_next_links_walks_a_backward_last_n_serving_every_instance_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let speed = (0..100_i32).map(|value| (value, value.unsigned_abs())).collect::<Vec<_>>();
    let fuel = (0..30_i32).map(|step| (100 + step, 2 * step.unsigned_abs())).collect::<Vec<_>>();
    let paged_series_backward_evolution = series(&namespace, "backward", &speed, &fuel)?;
    let paged_series_backward_id = paged_series_backward_evolution.id.clone();
    broker.temporal().create_or_update(&paged_series_backward_evolution).send().await?.created()?;
    let pages = [
        Page {
            from: 99,
            to: 81,
            size: 50,
            time_at: 81,
            last_n: 50,
            speed: 81..=99,
            fuel: None,
        },
        Page {
            from: 81,
            to: 62,
            size: 50,
            time_at: 62,
            last_n: 50,
            speed: 62..=80,
            fuel: None,
        },
        Page {
            from: 62,
            to: 43,
            size: 50,
            time_at: 43,
            last_n: 42,
            speed: 43..=61,
            fuel: Some(122..=129),
        },
        Page {
            from: 43,
            to: 24,
            size: 42,
            time_at: 24,
            last_n: 32,
            speed: 24..=42,
            fuel: Some(112..=121),
        },
        Page {
            from: 24,
            to: 5,
            size: 32,
            time_at: 5,
            last_n: 23,
            speed: 5..=23,
            fuel: Some(103..=111),
        },
    ];
    // A page can hold an attribute as an empty array, which the typed client refuses, so the first page is requested
    // through the operation and read as untyped JSON.
    let window = TemporalSelection::Unbounded {
        time_property: None,
        last_n: NonZeroU64::new(50),
        aggregation_methods: None,
        aggregation_period: None,
    };
    let operation = RetrieveTemporalEvolution::builder()
        .id(&paged_series_backward_id)
        .selection(window)
        .format(NormalizedTemporal)
        .build();
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let mut answer = client.send_unchecked(client.unchecked(&operation)?).await?;
    for page in &pages {
        expect_that!(answer.status(), eq(StatusCode::PARTIAL_CONTENT));
        holds_page(&answer, page)?;
        let link = ResponseHead::new(answer.headers().clone()).typed::<Link>().into_valid().or_fail()?;
        let request = followed_link::request(&broker, &link, &RelationType::Next)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
        answer = broker.send_unchecked(request).await?;
    }
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(answer.headers().get(CONTENT_RANGE), none());
    expect_that!(answer.headers().get(LINK), none());
    let last = read(&answer)?;
    holds_values(&last, 0..=4, Some(100..=102))?;
    Ok(())
}

/// What one partial page of the backward walk holds.
struct Page {
    /// Minute of the first instant of the page's `Content-Range`.
    from: u32,
    /// Minute of the last instant of the page's `Content-Range`.
    to: u32,
    /// The size the `Content-Range` states, the `lastN` that was asked.
    size: u64,
    /// Minute of the `timeAt` of the `rel="next"` Link.
    time_at: u32,
    /// The `lastN` of the `rel="next"` Link.
    last_n: u64,
    /// The `speed` values on the page.
    speed: RangeInclusive<i32>,
    /// The `fuel` values on the page; `None` when the page holds none.
    fuel: Option<RangeInclusive<i32>>,
}

/// Checks the head and the instances of one partial page.
fn holds_page(answer: &Response<Bytes>, page: &Page) -> Result<()> {
    let head = ResponseHead::new(answer.headers().clone());
    let size = ContentRangeSize::LastN(NonZeroU64::new(page.size).or_fail()?);
    verify_that!(head.typed::<ContentRange>().valid(), some(has_range(stamp(page.from)?, stamp(page.to)?, size)))?;
    let links = link_text(answer.headers());
    verify_that!(&links, contains_substring("timerel=before"))?;
    let time_at = format!("2024-01-01T{:02}%3A{:02}%3A00Z", page.time_at / 60, page.time_at % 60);
    verify_that!(&links, contains_substring(format!("timeAt={time_at}")))?;
    verify_that!(&links, contains_substring(format!("lastN={}", page.last_n)))?;
    holds_values(&read(answer)?, page.speed.clone(), page.fuel.clone())
}

/// Checks that `evolution` holds exactly the `speed` values of `speed` and the `fuel` values of `fuel`.
fn holds_values(evolution: &TemporalEntity<Output>, speed: RangeInclusive<i32>, fuel: Option<RangeInclusive<i32>>) -> Result<()> {
    verify_that!(evolution, has_evolution("speed", len(eq(speed.clone().count()))))?;
    for value in speed {
        verify_that!(evolution, has_evolution("speed", contains(any_attribute().value(value))))?;
    }
    let Some(fuel) = fuel else {
        return verify_that!(evolution, lacks_evolution("fuel"));
    };
    verify_that!(evolution, has_evolution("fuel", len(eq(fuel.clone().count()))))?;
    for value in fuel {
        verify_that!(evolution, has_evolution("fuel", contains(any_attribute().value(value))))?;
    }
    Ok(())
}
