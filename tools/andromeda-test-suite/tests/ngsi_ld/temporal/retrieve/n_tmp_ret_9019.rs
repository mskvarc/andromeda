//! Last n above the maximum is paged and within it answered in full.

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

/// Pins: Retrieve Temporal Evolution of an Entity with 30 instances and `lastN` 5000 answers 206 with the 19 most
/// recent and Content-Range `DateTime` from the most recent to the least recent of them with size 5000, and with
/// `lastN` 10 answers 200 with 10 instances and no Content-Range
/// Clause: 5.7.3.4, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: the base stack answers at most 19 instances per attribute without
/// `lastN` and with `lastN` (clause 6.3.10 leaves both to the implementation)
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9019_last_n_above_the_maximum_is_paged_and_within_it_answered_in_full() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let paged_series_maximum_id = namespace.urn("PagedSeries", "maximum")?;
    let paged_series_type = namespace.entity_type("PagedSeries")?;
    let paged_series_maximum_evolution = TemporalEntity::builder()
        .id(paged_series_maximum_id.clone())
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
    broker.temporal().create_or_update(&paged_series_maximum_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_maximum_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?).last_n(5000))
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T00:29:00Z".parse()?,
            "2024-01-01T00:11:00Z".parse()?,
            ContentRangeSize::LastN(5000.try_into()?)
        )?))
    );
    let retrieved = &headed.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(19)),
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
                contains(any_attribute().value(28)),
                contains(any_attribute().value(29))
            )
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&paged_series_maximum_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?).last_n(10))
        .send()
        .await?;
    expect_that!(headed_2.head.typed::<ContentRange>(), absent());
    let retrieved_2 = &headed_2.outcome;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", len(eq(10))));
    Ok(())
}
