//! A simultaneous group above the page is carried whole.

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

/// Pins: Retrieve Temporal Evolution of an Entity holding 25 instances observed at 00:00 and 30 more one minute
/// apart, timerel after without `lastN`, answers 206 with the 25 and Content-Range `DateTime` from `timeAt` to
/// 00:00, resumed at 00:00 answers 206 with the 25 again and the 19 to 00:19, and resumed at 00:19 answers 200 with
/// the 12 from 00:19
/// Clause: 4.11, 5.7.3.4, 6.3.10, 6.3.15
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// `lastN` and with `lastN` (clause 6.3.10 leaves both to the implementation)
/// Recorded: clause 6.3.10 resumes a page from a range-end `DateTime`, which cannot point inside an instant, so
/// Andromeda carries the instances sharing the instant at a page edge whole, past the page size, and leaves those
/// on the inclusive lower bound of a resumed forward page uncounted
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9021_a_simultaneous_group_above_the_page_is_carried_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let paged_series_oversized_group_id = namespace.urn("PagedSeries", "oversized-group")?;
    let paged_series_type = namespace.entity_type("PagedSeries")?;
    let paged_series_oversized_group_evolution = TemporalEntity::builder()
        .id(paged_series_oversized_group_id.clone())
        .entity_type(paged_series_type)
        .property("speed", Property::builder(0).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(7).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(8).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(9).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(11).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(12).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(13).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(14).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(15).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(16).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(17).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(18).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(19).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(23).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(24).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(1000).observed_at("2024-01-01T00:01:00Z".parse()?))
        .property("speed", Property::builder(1001).observed_at("2024-01-01T00:02:00Z".parse()?))
        .property("speed", Property::builder(1002).observed_at("2024-01-01T00:03:00Z".parse()?))
        .property("speed", Property::builder(1003).observed_at("2024-01-01T00:04:00Z".parse()?))
        .property("speed", Property::builder(1004).observed_at("2024-01-01T00:05:00Z".parse()?))
        .property("speed", Property::builder(1005).observed_at("2024-01-01T00:06:00Z".parse()?))
        .property("speed", Property::builder(1006).observed_at("2024-01-01T00:07:00Z".parse()?))
        .property("speed", Property::builder(1007).observed_at("2024-01-01T00:08:00Z".parse()?))
        .property("speed", Property::builder(1008).observed_at("2024-01-01T00:09:00Z".parse()?))
        .property("speed", Property::builder(1009).observed_at("2024-01-01T00:10:00Z".parse()?))
        .property("speed", Property::builder(1010).observed_at("2024-01-01T00:11:00Z".parse()?))
        .property("speed", Property::builder(1011).observed_at("2024-01-01T00:12:00Z".parse()?))
        .property("speed", Property::builder(1012).observed_at("2024-01-01T00:13:00Z".parse()?))
        .property("speed", Property::builder(1013).observed_at("2024-01-01T00:14:00Z".parse()?))
        .property("speed", Property::builder(1014).observed_at("2024-01-01T00:15:00Z".parse()?))
        .property("speed", Property::builder(1015).observed_at("2024-01-01T00:16:00Z".parse()?))
        .property("speed", Property::builder(1016).observed_at("2024-01-01T00:17:00Z".parse()?))
        .property("speed", Property::builder(1017).observed_at("2024-01-01T00:18:00Z".parse()?))
        .property("speed", Property::builder(1018).observed_at("2024-01-01T00:19:00Z".parse()?))
        .property("speed", Property::builder(1019).observed_at("2024-01-01T00:20:00Z".parse()?))
        .property("speed", Property::builder(1020).observed_at("2024-01-01T00:21:00Z".parse()?))
        .property("speed", Property::builder(1021).observed_at("2024-01-01T00:22:00Z".parse()?))
        .property("speed", Property::builder(1022).observed_at("2024-01-01T00:23:00Z".parse()?))
        .property("speed", Property::builder(1023).observed_at("2024-01-01T00:24:00Z".parse()?))
        .property("speed", Property::builder(1024).observed_at("2024-01-01T00:25:00Z".parse()?))
        .property("speed", Property::builder(1025).observed_at("2024-01-01T00:26:00Z".parse()?))
        .property("speed", Property::builder(1026).observed_at("2024-01-01T00:27:00Z".parse()?))
        .property("speed", Property::builder(1027).observed_at("2024-01-01T00:28:00Z".parse()?))
        .property("speed", Property::builder(1028).observed_at("2024-01-01T00:29:00Z".parse()?))
        .property("speed", Property::builder(1029).observed_at("2024-01-01T00:30:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&paged_series_oversized_group_evolution)
        .send()
        .await?
        .created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_oversized_group_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2020-01-01T00:00:00Z".parse()?,
            "2024-01-01T00:00:00Z".parse()?,
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
                len(eq(25)),
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
                not(contains(any_attribute().value(1000)))
            )
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_oversized_group_id)
        .temporal_q(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(headed_2.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_2.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T00:00:00Z".parse()?,
            "2024-01-01T00:19:00Z".parse()?,
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
                len(eq(44)),
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
                contains(any_attribute().value(1000)),
                contains(any_attribute().value(1001)),
                contains(any_attribute().value(1002)),
                contains(any_attribute().value(1003)),
                contains(any_attribute().value(1004)),
                contains(any_attribute().value(1005)),
                contains(any_attribute().value(1006)),
                contains(any_attribute().value(1007)),
                contains(any_attribute().value(1008)),
                contains(any_attribute().value(1009)),
                contains(any_attribute().value(1010)),
                contains(any_attribute().value(1011)),
                contains(any_attribute().value(1012)),
                contains(any_attribute().value(1013)),
                contains(any_attribute().value(1014)),
                contains(any_attribute().value(1015)),
                contains(any_attribute().value(1016)),
                contains(any_attribute().value(1017)),
                contains(any_attribute().value(1018)),
                not(contains(any_attribute().value(1019)))
            )
        )
    );
    let headed_3 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_oversized_group_id)
        .temporal_q(temporal_query::after("2024-01-01T00:19:00Z".parse()?))
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
                len(eq(12)),
                contains(any_attribute().value(1018)),
                contains(any_attribute().value(1019)),
                contains(any_attribute().value(1020)),
                contains(any_attribute().value(1021)),
                contains(any_attribute().value(1022)),
                contains(any_attribute().value(1023)),
                contains(any_attribute().value(1024)),
                contains(any_attribute().value(1025)),
                contains(any_attribute().value(1026)),
                contains(any_attribute().value(1027)),
                contains(any_attribute().value(1028)),
                contains(any_attribute().value(1029))
            )
        )
    );
    Ok(())
}
