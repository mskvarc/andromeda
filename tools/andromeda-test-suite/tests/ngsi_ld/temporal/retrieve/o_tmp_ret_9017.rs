//! A backward walk collects every instance exactly once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    header_matcher::absent,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize},
    model::{DateTime, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: A Temporal Evolution read backwards with `lastN` 1000, each page resumed with timerel before at the
/// previous range-end, answers 206 pages with a backward Content-Range of size 1000 and then 200, together holding
/// every instance once
/// Clause: 4.11, 5.7.3.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// `lastN` and with `lastN` (clause 6.3.10 leaves both to the implementation)
/// Recorded: clause 6.3.10 does not say where a page is cut; Andromeda cuts every attribute of a response at one
/// instant and keeps the instances sharing that instant together, so a forward walk resumed with the inclusive
/// after meets that group again under the same `instanceIds`
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9017_a_backward_walk_collects_every_instance_exactly_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let paged_series_backward_id = namespace.urn("PagedSeries", "backward")?;
    let paged_series_type = namespace.entity_type("PagedSeries")?;
    let paged_series_backward_evolution = TemporalEntity::builder()
        .id(paged_series_backward_id.clone())
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
        .property("speed", Property::builder(19).observed_at("2024-01-01T00:18:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:18:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2024-01-01T00:19:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2024-01-01T00:20:00Z".parse()?))
        .property("speed", Property::builder(23).observed_at("2024-01-01T00:21:00Z".parse()?))
        .property("speed", Property::builder(24).observed_at("2024-01-01T00:22:00Z".parse()?))
        .property("speed", Property::builder(25).observed_at("2024-01-01T00:23:00Z".parse()?))
        .property("speed", Property::builder(26).observed_at("2024-01-01T00:24:00Z".parse()?))
        .property("speed", Property::builder(27).observed_at("2024-01-01T00:25:00Z".parse()?))
        .property("speed", Property::builder(28).observed_at("2024-01-01T00:26:00Z".parse()?))
        .property("speed", Property::builder(29).observed_at("2024-01-01T00:27:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-01T00:28:00Z".parse()?))
        .property("speed", Property::builder(31).observed_at("2024-01-01T00:29:00Z".parse()?))
        .property("speed", Property::builder(32).observed_at("2024-01-01T00:30:00Z".parse()?))
        .property("speed", Property::builder(33).observed_at("2024-01-01T00:31:00Z".parse()?))
        .property("speed", Property::builder(34).observed_at("2024-01-01T00:32:00Z".parse()?))
        .property("speed", Property::builder(35).observed_at("2024-01-01T00:33:00Z".parse()?))
        .property("speed", Property::builder(36).observed_at("2024-01-01T00:34:00Z".parse()?))
        .property("speed", Property::builder(37).observed_at("2024-01-01T00:34:00Z".parse()?))
        .property("speed", Property::builder(38).observed_at("2024-01-01T00:35:00Z".parse()?))
        .property("speed", Property::builder(39).observed_at("2024-01-01T00:36:00Z".parse()?))
        .property("speed", Property::builder(40).observed_at("2024-01-01T00:37:00Z".parse()?))
        .property("speed", Property::builder(41).observed_at("2024-01-01T00:38:00Z".parse()?))
        .property("speed", Property::builder(42).observed_at("2024-01-01T00:39:00Z".parse()?))
        .property("speed", Property::builder(43).observed_at("2024-01-01T00:40:00Z".parse()?))
        .property("speed", Property::builder(44).observed_at("2024-01-01T00:41:00Z".parse()?))
        .property("speed", Property::builder(45).observed_at("2024-01-01T00:42:00Z".parse()?))
        .property("speed", Property::builder(46).observed_at("2024-01-01T00:43:00Z".parse()?))
        .property("speed", Property::builder(47).observed_at("2024-01-01T00:44:00Z".parse()?))
        .property("fuel", Property::builder(100).observed_at("2024-01-01T00:01:00Z".parse()?))
        .property("fuel", Property::builder(101).observed_at("2024-01-01T00:03:00Z".parse()?))
        .property("fuel", Property::builder(102).observed_at("2024-01-01T00:05:00Z".parse()?))
        .property("fuel", Property::builder(103).observed_at("2024-01-01T00:07:00Z".parse()?))
        .property("fuel", Property::builder(104).observed_at("2024-01-01T00:09:00Z".parse()?))
        .property("fuel", Property::builder(105).observed_at("2024-01-01T00:11:00Z".parse()?))
        .property("fuel", Property::builder(106).observed_at("2024-01-01T00:13:00Z".parse()?))
        .property("fuel", Property::builder(107).observed_at("2024-01-01T00:15:00Z".parse()?))
        .property("fuel", Property::builder(108).observed_at("2024-01-01T00:17:00Z".parse()?))
        .property("fuel", Property::builder(109).observed_at("2024-01-01T00:19:00Z".parse()?))
        .property("fuel", Property::builder(110).observed_at("2024-01-01T00:21:00Z".parse()?))
        .property("fuel", Property::builder(111).observed_at("2024-01-01T00:23:00Z".parse()?))
        .property("fuel", Property::builder(112).observed_at("2024-01-01T00:25:00Z".parse()?))
        .property("fuel", Property::builder(113).observed_at("2024-01-01T00:27:00Z".parse()?))
        .property("fuel", Property::builder(114).observed_at("2024-01-01T00:29:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_series_backward_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_backward_id)
        .temporal_q(temporal_query::before("2024-01-02T00:00:00Z".parse()?).last_n(1000))
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-02T00:00:00Z".parse()?,
            "2024-01-01T00:27:00Z".parse()?,
            ContentRangeSize::LastN(1000.try_into()?)
        )?))
    );
    let end_0 = *headed.head.typed::<ContentRange>().valid().or_fail()?.end();
    let retrieved = &headed.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(19)),
                contains(any_attribute().value(29)),
                contains(any_attribute().value(30)),
                contains(any_attribute().value(31)),
                contains(any_attribute().value(32)),
                contains(any_attribute().value(33)),
                contains(any_attribute().value(34)),
                contains(any_attribute().value(35)),
                contains(any_attribute().value(36)),
                contains(any_attribute().value(37)),
                contains(any_attribute().value(38)),
                contains(any_attribute().value(39)),
                contains(any_attribute().value(40)),
                contains(any_attribute().value(41)),
                contains(any_attribute().value(42)),
                contains(any_attribute().value(43)),
                contains(any_attribute().value(44)),
                contains(any_attribute().value(45)),
                contains(any_attribute().value(46)),
                contains(any_attribute().value(47))
            )
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "fuel",
            all!(len(eq(2)), contains(any_attribute().value(113)), contains(any_attribute().value(114)))
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_backward_id)
        .temporal_q(temporal_query::before(end_0).last_n(1000))
        .send()
        .await?;
    expect_that!(headed_2.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_2
            .head
            .typed::<ContentRange>()
            .valid()
            .map(|range| (range.start().timestamp(), range.end().timestamp(), range.size())),
        some(eq((
            end_0.timestamp(),
            "2024-01-01T00:10:00Z".parse::<DateTime>()?.timestamp(),
            ContentRangeSize::LastN(1000.try_into()?)
        )))
    );
    let end_1 = *headed_2.head.typed::<ContentRange>().valid().or_fail()?.end();
    let retrieved_2 = &headed_2.outcome;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(
                len(eq(19)),
                contains(any_attribute().value(10)),
                contains(any_attribute().value(11)),
                contains(any_attribute().value(12)),
                contains(any_attribute().value(13)),
                contains(any_attribute().value(14)),
                contains(any_attribute().value(15)),
                contains(any_attribute().value(16)),
                contains(any_attribute().value(17)),
                contains(any_attribute().value(18)),
                contains(any_attribute().value(19)),
                contains(any_attribute().value(20)),
                contains(any_attribute().value(21)),
                contains(any_attribute().value(22)),
                contains(any_attribute().value(23)),
                contains(any_attribute().value(24)),
                contains(any_attribute().value(25)),
                contains(any_attribute().value(26)),
                contains(any_attribute().value(27)),
                contains(any_attribute().value(28))
            )
        )
    );
    expect_that!(
        evolution_2,
        has_evolution(
            "fuel",
            all!(
                len(eq(8)),
                contains(any_attribute().value(105)),
                contains(any_attribute().value(106)),
                contains(any_attribute().value(107)),
                contains(any_attribute().value(108)),
                contains(any_attribute().value(109)),
                contains(any_attribute().value(110)),
                contains(any_attribute().value(111)),
                contains(any_attribute().value(112))
            )
        )
    );
    let headed_3 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_backward_id)
        .temporal_q(temporal_query::before(end_1).last_n(1000))
        .send()
        .await?;
    expect_that!(headed_3.head.typed::<ContentRange>(), absent());
    let retrieved_3 = &headed_3.outcome;
    let evolution_3 = retrieved_3.body();
    expect_that!(
        evolution_3,
        has_evolution(
            "speed",
            all!(
                len(eq(10)),
                contains(any_attribute().value(0)),
                contains(any_attribute().value(1)),
                contains(any_attribute().value(2)),
                contains(any_attribute().value(3)),
                contains(any_attribute().value(4)),
                contains(any_attribute().value(5)),
                contains(any_attribute().value(6)),
                contains(any_attribute().value(7)),
                contains(any_attribute().value(8)),
                contains(any_attribute().value(9))
            )
        )
    );
    expect_that!(
        evolution_3,
        has_evolution(
            "fuel",
            all!(
                len(eq(5)),
                contains(any_attribute().value(100)),
                contains(any_attribute().value(101)),
                contains(any_attribute().value(102)),
                contains(any_attribute().value(103)),
                contains(any_attribute().value(104))
            )
        )
    );
    Ok(())
}
