//! A page keeps the instances sharing its edge instant together.

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
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity with 40 speed instances, the 19th and 20th sharing 00:18, answers
/// a forward page of the 20 instances up to 00:18, a page resumed at 00:18 of those two and the next 19, the last
/// page of the remaining two, `lastN` 3 of the three latest, and `lastN` 5000 before 00:20 of the 19 latest from
/// 00:02
/// Clause: 4.11, 5.7.3.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// `lastN` and with `lastN` (clause 6.3.10 leaves both to the implementation); a page keeps the instances sharing
/// the instant at its edge together, so a client resuming from its range-end neither skips nor splits them
#[gtest]
#[tokio::test]
async fn n_tmp_ret_0004_a_page_keeps_the_instances_sharing_its_edge_instant_together() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let paged_series_edge_id = namespace.urn("PagedSeries", "edge")?;
    let paged_series_type = namespace.entity_type("PagedSeries")?;
    let paged_series_edge_evolution = TemporalEntity::builder()
        .id(paged_series_edge_id.clone())
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
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:19:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2024-01-01T00:20:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2024-01-01T00:21:00Z".parse()?))
        .property("speed", Property::builder(23).observed_at("2024-01-01T00:22:00Z".parse()?))
        .property("speed", Property::builder(24).observed_at("2024-01-01T00:23:00Z".parse()?))
        .property("speed", Property::builder(25).observed_at("2024-01-01T00:24:00Z".parse()?))
        .property("speed", Property::builder(26).observed_at("2024-01-01T00:25:00Z".parse()?))
        .property("speed", Property::builder(27).observed_at("2024-01-01T00:26:00Z".parse()?))
        .property("speed", Property::builder(28).observed_at("2024-01-01T00:27:00Z".parse()?))
        .property("speed", Property::builder(29).observed_at("2024-01-01T00:28:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-01T00:29:00Z".parse()?))
        .property("speed", Property::builder(31).observed_at("2024-01-01T00:30:00Z".parse()?))
        .property("speed", Property::builder(32).observed_at("2024-01-01T00:31:00Z".parse()?))
        .property("speed", Property::builder(33).observed_at("2024-01-01T00:32:00Z".parse()?))
        .property("speed", Property::builder(34).observed_at("2024-01-01T00:33:00Z".parse()?))
        .property("speed", Property::builder(35).observed_at("2024-01-01T00:34:00Z".parse()?))
        .property("speed", Property::builder(36).observed_at("2024-01-01T00:35:00Z".parse()?))
        .property("speed", Property::builder(37).observed_at("2024-01-01T00:36:00Z".parse()?))
        .property("speed", Property::builder(38).observed_at("2024-01-01T00:37:00Z".parse()?))
        .property("speed", Property::builder(39).observed_at("2024-01-01T00:38:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_series_edge_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_edge_id)
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
    let retrieved = &headed.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(20)),
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
                not(contains(any_attribute().value(20)))
            )
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_edge_id)
        .temporal_q(temporal_query::after("2024-01-01T00:18:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed_2.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_2.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T00:18:00Z".parse()?,
            "2024-01-01T00:37:00Z".parse()?,
            ContentRangeSize::Unknown
        )?))
    );
    let retrieved_2 = &headed_2.outcome;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(
                len(eq(21)),
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
                contains(any_attribute().value(28)),
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
                not(contains(any_attribute().value(17))),
                not(contains(any_attribute().value(39)))
            )
        )
    );
    let headed_3 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_edge_id)
        .temporal_q(temporal_query::after("2024-01-01T00:37:00Z".parse()?))
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
                len(eq(2)),
                contains(any_attribute().value(38)),
                contains(any_attribute().value(39)),
                not(contains(any_attribute().value(37)))
            )
        )
    );
    let headed_4 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_edge_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).last_n(3))
        .send()
        .await?;
    expect_that!(headed_4.head.typed::<ContentRange>(), absent());
    let retrieved_4 = &headed_4.outcome;
    let evolution_4 = retrieved_4.body();
    expect_that!(
        evolution_4,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                contains(any_attribute().value(37)),
                contains(any_attribute().value(38)),
                contains(any_attribute().value(39)),
                not(contains(any_attribute().value(36)))
            )
        )
    );
    let headed_5 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_edge_id)
        .temporal_q(temporal_query::before("2024-01-01T00:20:00Z".parse()?).last_n(5000))
        .send()
        .await?;
    expect_that!(headed_5.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_5.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T00:20:00Z".parse()?,
            "2024-01-01T00:02:00Z".parse()?,
            ContentRangeSize::LastN(5000.try_into()?)
        )?))
    );
    let retrieved_5 = &headed_5.outcome;
    let evolution_5 = retrieved_5.body();
    expect_that!(
        evolution_5,
        has_evolution(
            "speed",
            all!(
                len(eq(19)),
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
                not(contains(any_attribute().value(1))),
                not(contains(any_attribute().value(21)))
            )
        )
    );
    Ok(())
}
