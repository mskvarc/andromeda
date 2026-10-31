//! The content range follows the direction and the timerel.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: A Temporal Evolution of 30 instances read with after, before and between without `lastN` answers a
/// Content-Range from `timeAt`, the least recent instant and `timeAt`, and with `lastN` 25 from `timeAt`,
/// `endTimeAt` and the most recent instant, each to the edge of its page
/// Clause: 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// `lastN` and with `lastN` (clause 6.3.10 leaves both to the implementation)
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9020_the_content_range_follows_the_direction_and_the_timerel() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let paged_series_content_range_id = namespace.urn("PagedSeries", "content-range")?;
    let paged_series_type = namespace.entity_type("PagedSeries")?;
    let paged_series_content_range_evolution = TemporalEntity::builder()
        .id(paged_series_content_range_id.clone())
        .entity_type(paged_series_type)
        .property("speed", Property::builder(0).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:01:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-01T00:02:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2024-01-01T00:03:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2024-01-01T00:04:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2024-01-01T00:05:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2024-01-01T00:06:00Z".parse()?))
        .property("speed", Property::builder(7).observed_at("2024-01-01T00:07:00Z".parse()?))
        .property("speed", Property::builder(8).observed_at("2024-01-01T00:08:00Z".parse()?))
        .property("speed", Property::builder(9).observed_at("2024-01-01T00:09:00Z".parse()?))
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:10:00Z".parse()?))
        .property("speed", Property::builder(11).observed_at("2024-01-01T00:11:00Z".parse()?))
        .property("speed", Property::builder(12).observed_at("2024-01-01T00:12:00Z".parse()?))
        .property("speed", Property::builder(13).observed_at("2024-01-01T00:13:00Z".parse()?))
        .property("speed", Property::builder(14).observed_at("2024-01-01T00:14:00Z".parse()?))
        .property("speed", Property::builder(15).observed_at("2024-01-01T00:15:00Z".parse()?))
        .property("speed", Property::builder(16).observed_at("2024-01-01T00:16:00Z".parse()?))
        .property("speed", Property::builder(17).observed_at("2024-01-01T00:17:00Z".parse()?))
        .property("speed", Property::builder(18).observed_at("2024-01-01T00:18:00Z".parse()?))
        .property("speed", Property::builder(19).observed_at("2024-01-01T00:19:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:20:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2024-01-01T00:21:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2024-01-01T00:22:00Z".parse()?))
        .property("speed", Property::builder(23).observed_at("2024-01-01T00:23:00Z".parse()?))
        .property("speed", Property::builder(24).observed_at("2024-01-01T00:24:00Z".parse()?))
        .property("speed", Property::builder(25).observed_at("2024-01-01T00:25:00Z".parse()?))
        .property("speed", Property::builder(26).observed_at("2024-01-01T00:26:00Z".parse()?))
        .property("speed", Property::builder(27).observed_at("2024-01-01T00:27:00Z".parse()?))
        .property("speed", Property::builder(28).observed_at("2024-01-01T00:28:00Z".parse()?))
        .property("speed", Property::builder(29).observed_at("2024-01-01T00:29:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&paged_series_content_range_evolution)
        .send()
        .await?
        .created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_content_range_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2023-12-31T00:00:00Z".parse()?,
            "2024-01-01T00:18:00Z".parse()?,
            ContentRangeSize::Unknown
        )?))
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_content_range_id)
        .temporal_q(temporal_query::before("2024-01-02T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed_2.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_2.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T00:00:00Z".parse()?,
            "2024-01-01T00:18:00Z".parse()?,
            ContentRangeSize::Unknown
        )?))
    );
    let headed_3 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_content_range_id)
        .temporal_q(temporal_query::between("2023-12-31T00:00:00Z".parse()?, "2024-01-02T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed_3.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_3.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2023-12-31T00:00:00Z".parse()?,
            "2024-01-01T00:18:00Z".parse()?,
            ContentRangeSize::Unknown
        )?))
    );
    let headed_4 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_content_range_id)
        .temporal_q(temporal_query::before("2024-01-02T00:00:00Z".parse()?).last_n(25))
        .send()
        .await?;
    expect_that!(headed_4.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_4.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-02T00:00:00Z".parse()?,
            "2024-01-01T00:11:00Z".parse()?,
            ContentRangeSize::LastN(25.try_into()?)
        )?))
    );
    let headed_5 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_content_range_id)
        .temporal_q(temporal_query::between("2023-12-31T00:00:00Z".parse()?, "2024-01-02T00:00:00Z".parse()?).last_n(25))
        .send()
        .await?;
    expect_that!(headed_5.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_5.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-02T00:00:00Z".parse()?,
            "2024-01-01T00:11:00Z".parse()?,
            ContentRangeSize::LastN(25.try_into()?)
        )?))
    );
    let headed_6 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_content_range_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).last_n(25))
        .send()
        .await?;
    expect_that!(headed_6.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_6.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T00:29:00Z".parse()?,
            "2024-01-01T00:11:00Z".parse()?,
            ContentRangeSize::LastN(25.try_into()?)
        )?))
    );
    Ok(())
}
