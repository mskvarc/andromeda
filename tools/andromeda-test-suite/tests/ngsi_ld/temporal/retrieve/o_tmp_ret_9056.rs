//! A paged forward walk answers no instance without observed at.

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

/// Pins: Retrieve Temporal Evolution of speed holding 25 instances observed each minute from 2024-01-01T00:00:00Z
/// and two with no `observedAt` answers without `lastN` with 206 and the 19 oldest observed instances, and the page
/// resumed after 2024-01-01T00:18:00Z with 200 and the instances from 18 on: the pages together hold the 25
/// observed instances, the instances a page holding the whole history would, and no instance with no `observedAt`
/// Clause: 5.7.3.4, 6.3.10, 4.11, 4.5.7
/// Recorded: Andromeda reading of v1.9.1: clause 6.3.10 orders a forward page by timestamp and clause 4.5.7 places
/// every instance at the instant of its Temporal Property, so an instance with no value on the requested property
/// is left out of every read on it, the first page as much as the resumed one (clause 4.11); the base stack answers
/// at most 19 instances per attribute (clause 6.3.10 leaves the limit to the implementation)
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9056_a_paged_forward_walk_answers_no_instance_without_observed_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let series_id = namespace.urn("Series", 1)?;
    let series_type = namespace.entity_type("Series")?;
    let series_evolution = TemporalEntity::builder()
        .id(series_id.clone())
        .entity_type(series_type)
        .property("speed", Property::builder(0).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:01:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-01T00:02:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2024-01-01T00:03:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2024-01-01T00:04:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2024-01-01T00:05:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2024-01-01T00:06:00Z".parse()?))
        .property("speed", Property::builder(7).observed_at("2024-01-01T00:07:00Z".parse()?))
        .property("speed", 1000)
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
        .property("speed", 2000)
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:20:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2024-01-01T00:21:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2024-01-01T00:22:00Z".parse()?))
        .property("speed", Property::builder(23).observed_at("2024-01-01T00:23:00Z".parse()?))
        .property("speed", Property::builder(24).observed_at("2024-01-01T00:24:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&series_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&series_id)
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(headed.head.typed::<ContentRange>().valid(), some(anything()));
    expect_that!(
        headed.head.typed::<ContentRange>().valid().map(|range| range.end().timestamp()),
        some(eq("2024-01-01T00:18:00Z".parse::<DateTime>()?.timestamp()))
    );
    expect_that!(
        headed.head.typed::<ContentRange>().valid().map(ContentRange::size),
        some(eq(ContentRangeSize::Unknown))
    );
    let retrieved = &headed.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(19)),
                contains(any_attribute().value(0)),
                contains(any_attribute().value(1)),
                contains(any_attribute().value(2)),
                contains(any_attribute().value(3)),
                contains(any_attribute().value(4)),
                contains(any_attribute().value(5)),
                contains(any_attribute().value(6)),
                contains(any_attribute().value(7)),
                contains(any_attribute().value(8)),
                contains(any_attribute().value(9)),
                contains(any_attribute().value(10)),
                contains(any_attribute().value(11)),
                contains(any_attribute().value(12)),
                contains(any_attribute().value(13)),
                contains(any_attribute().value(14)),
                contains(any_attribute().value(15)),
                contains(any_attribute().value(16)),
                contains(any_attribute().value(17)),
                contains(any_attribute().value(18)),
                not(contains(any_attribute().value(1000))),
                not(contains(any_attribute().value(2000)))
            )
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&series_id)
        .temporal_q(temporal_query::after("2024-01-01T00:18:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed_2.head.typed::<ContentRange>(), absent());
    let retrieved_2 = &headed_2.outcome;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(
                len(eq(7)),
                contains(any_attribute().value(18)),
                contains(any_attribute().value(19)),
                contains(any_attribute().value(20)),
                contains(any_attribute().value(21)),
                contains(any_attribute().value(22)),
                contains(any_attribute().value(23)),
                contains(any_attribute().value(24)),
                not(contains(any_attribute().value(1000))),
                not(contains(any_attribute().value(2000)))
            )
        )
    );
    Ok(())
}
