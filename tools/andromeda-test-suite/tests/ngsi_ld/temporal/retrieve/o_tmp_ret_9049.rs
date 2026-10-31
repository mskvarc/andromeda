//! A backward page carries the write it reaches into whole.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    header_matcher::absent,
    instance_time::instance_time_where,
    namespace::Namespace,
    outcome::Upserted,
    received_status::ExpectedStatus,
    system_time::SystemMember,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, ContentRange, ContentRangeSize},
        operation::SystemAttributes,
    },
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Retrieve Temporal Evolution of an Entity written by a temporal create of 3 speed instances, then a
/// temporal append of 25 and one of 5, on `createdAt` after 2026-01-01 with `lastN` 21 answers 206 with the 5 of
/// the last write and the 25 of the one before, Content-Range `DateTime` from the last write's `createdAt` to the
/// one before's with size 21, and resumed before that instant answers 200 with the 3 of the first write
/// Clause: 4.8, 4.11, 5.6.12, 5.7.3.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// `lastN` and with `lastN` (clause 6.3.10 leaves both to the implementation)
/// Recorded: one temporal write stamps every instance it records with one `createdAt`; clause 6.3.10 resumes a page
/// from a range-end `DateTime`, which cannot point inside an instant, so Andromeda carries the instances sharing
/// the instant at a page edge whole, past the page size, and leaves those on the inclusive lower bound of a resumed
/// forward page uncounted
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9049_a_backward_page_carries_the_write_it_reaches_into_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let paged_series_backward_id = namespace.urn("PagedSeries", "backward")?;
    let paged_series_type = namespace.entity_type("PagedSeries")?;
    let paged_series_backward_evolution = TemporalEntity::builder()
        .id(paged_series_backward_id.clone())
        .entity_type(paged_series_type.clone())
        .property("speed", Property::builder(200).observed_at("2023-12-31T00:00:00Z".parse()?))
        .property("speed", Property::builder(201).observed_at("2023-12-31T00:01:00Z".parse()?))
        .property("speed", Property::builder(202).observed_at("2023-12-31T00:02:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_series_backward_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(paged_series_backward_id.clone())
        .entity_type(paged_series_type.clone())
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
        .build()?
        .attributes;
    broker.temporal().add_attributes(&paged_series_backward_id, &instances).send().await?;
    let instances_2 = TemporalEntity::builder()
        .id(paged_series_backward_id.clone())
        .entity_type(paged_series_type.clone())
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:30:00Z".parse()?))
        .property("speed", Property::builder(101).observed_at("2024-01-01T00:31:00Z".parse()?))
        .property("speed", Property::builder(102).observed_at("2024-01-01T00:32:00Z".parse()?))
        .property("speed", Property::builder(103).observed_at("2024-01-01T00:33:00Z".parse()?))
        .property("speed", Property::builder(104).observed_at("2024-01-01T00:34:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&paged_series_backward_id, &instances_2).send().await?;
    let retrieved = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_backward_id)
        .temporal_q(
            temporal_query::after("2026-01-01T00:00:00Z".parse()?)
                .time_property(TemporalProperty::CreatedAt)
                .last_n(21),
        )
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?
        .expecting(StatusCode::PARTIAL_CONTENT)?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(30)),
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
                contains(any_attribute().value(19)),
                contains(any_attribute().value(20)),
                contains(any_attribute().value(21)),
                contains(any_attribute().value(22)),
                contains(any_attribute().value(23)),
                contains(any_attribute().value(24)),
                contains(any_attribute().value(100)),
                contains(any_attribute().value(101)),
                contains(any_attribute().value(102)),
                contains(any_attribute().value(103)),
                contains(any_attribute().value(104)),
                not(contains(any_attribute().value(200)))
            )
        )
    );
    let second_write = instance_time_where(evolution, &"speed".parse()?, any_attribute().value(0), SystemMember::CreatedAt)?;
    let third_write = instance_time_where(evolution, &"speed".parse()?, any_attribute().value(100), SystemMember::CreatedAt)?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_backward_id)
        .temporal_q(
            temporal_query::after("2026-01-01T00:00:00Z".parse()?)
                .time_property(TemporalProperty::CreatedAt)
                .last_n(21),
        )
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed
            .head
            .typed::<ContentRange>()
            .valid()
            .map(|range| (range.start().timestamp(), range.end().timestamp(), range.size())),
        some(eq((third_write.timestamp(), second_write.timestamp(), ContentRangeSize::LastN(21.try_into()?))))
    );
    let retrieved_2 = &headed.outcome;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", len(eq(30))));
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_backward_id)
        .temporal_q(temporal_query::before(second_write).time_property(TemporalProperty::CreatedAt).last_n(21))
        .send()
        .await?;
    expect_that!(headed_2.head.typed::<ContentRange>(), absent());
    let retrieved_3 = &headed_2.outcome;
    let evolution_3 = retrieved_3.body();
    expect_that!(
        evolution_3,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                contains(any_attribute().value(200)),
                contains(any_attribute().value(201)),
                contains(any_attribute().value(202))
            )
        )
    );
    Ok(())
}
