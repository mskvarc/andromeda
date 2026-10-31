//! Every entity of an aggregated query is cut at the earliest period boundary.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, ContentRange, ContentRangeSize},
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: a query over two Entities of 40 hourly instances, one from 00:00 and one from 05:00, aggregated per PT1H
/// answers 206 with Content-Range `DateTime` 2024-01-01T00:00:00Z-2024-01-01T19:00:00Z/*: the first Entity holds
/// its 19 periods, the second the 14 periods ending by 19:00
/// Clause: 4.5.19, 6.3.10
/// Recorded: one response carries one Content-Range, which the client resumes the whole response from, so every
/// stream of every Entity is cut at the earliest period boundary any of them was cut at
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9026_every_entity_of_an_aggregated_query_is_cut_at_the_earliest_period_boundary() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_2_id = namespace.urn("Agg", 2)?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type.clone())
        .property("speed", Property::builder(0).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(1).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-01T02:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2024-01-01T03:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2024-01-01T04:00:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2024-01-01T05:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2024-01-01T06:00:00Z".parse()?))
        .property("speed", Property::builder(7).observed_at("2024-01-01T07:00:00Z".parse()?))
        .property("speed", Property::builder(8).observed_at("2024-01-01T08:00:00Z".parse()?))
        .property("speed", Property::builder(9).observed_at("2024-01-01T09:00:00Z".parse()?))
        .property("speed", Property::builder(10).observed_at("2024-01-01T10:00:00Z".parse()?))
        .property("speed", Property::builder(11).observed_at("2024-01-01T11:00:00Z".parse()?))
        .property("speed", Property::builder(12).observed_at("2024-01-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(13).observed_at("2024-01-01T13:00:00Z".parse()?))
        .property("speed", Property::builder(14).observed_at("2024-01-01T14:00:00Z".parse()?))
        .property("speed", Property::builder(15).observed_at("2024-01-01T15:00:00Z".parse()?))
        .property("speed", Property::builder(16).observed_at("2024-01-01T16:00:00Z".parse()?))
        .property("speed", Property::builder(17).observed_at("2024-01-01T17:00:00Z".parse()?))
        .property("speed", Property::builder(18).observed_at("2024-01-01T18:00:00Z".parse()?))
        .property("speed", Property::builder(19).observed_at("2024-01-01T19:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T20:00:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2024-01-01T21:00:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2024-01-01T22:00:00Z".parse()?))
        .property("speed", Property::builder(23).observed_at("2024-01-01T23:00:00Z".parse()?))
        .property("speed", Property::builder(24).observed_at("2024-01-02T00:00:00Z".parse()?))
        .property("speed", Property::builder(25).observed_at("2024-01-02T01:00:00Z".parse()?))
        .property("speed", Property::builder(26).observed_at("2024-01-02T02:00:00Z".parse()?))
        .property("speed", Property::builder(27).observed_at("2024-01-02T03:00:00Z".parse()?))
        .property("speed", Property::builder(28).observed_at("2024-01-02T04:00:00Z".parse()?))
        .property("speed", Property::builder(29).observed_at("2024-01-02T05:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-02T06:00:00Z".parse()?))
        .property("speed", Property::builder(31).observed_at("2024-01-02T07:00:00Z".parse()?))
        .property("speed", Property::builder(32).observed_at("2024-01-02T08:00:00Z".parse()?))
        .property("speed", Property::builder(33).observed_at("2024-01-02T09:00:00Z".parse()?))
        .property("speed", Property::builder(34).observed_at("2024-01-02T10:00:00Z".parse()?))
        .property("speed", Property::builder(35).observed_at("2024-01-02T11:00:00Z".parse()?))
        .property("speed", Property::builder(36).observed_at("2024-01-02T12:00:00Z".parse()?))
        .property("speed", Property::builder(37).observed_at("2024-01-02T13:00:00Z".parse()?))
        .property("speed", Property::builder(38).observed_at("2024-01-02T14:00:00Z".parse()?))
        .property("speed", Property::builder(39).observed_at("2024-01-02T15:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let agg_2_evolution = TemporalEntity::builder()
        .id(agg_2_id.clone())
        .entity_type(agg_type.clone())
        .property("speed", Property::builder(0).observed_at("2024-01-01T05:00:00Z".parse()?))
        .property("speed", Property::builder(1).observed_at("2024-01-01T06:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2024-01-01T07:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2024-01-01T08:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2024-01-01T09:00:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2024-01-01T10:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2024-01-01T11:00:00Z".parse()?))
        .property("speed", Property::builder(7).observed_at("2024-01-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(8).observed_at("2024-01-01T13:00:00Z".parse()?))
        .property("speed", Property::builder(9).observed_at("2024-01-01T14:00:00Z".parse()?))
        .property("speed", Property::builder(10).observed_at("2024-01-01T15:00:00Z".parse()?))
        .property("speed", Property::builder(11).observed_at("2024-01-01T16:00:00Z".parse()?))
        .property("speed", Property::builder(12).observed_at("2024-01-01T17:00:00Z".parse()?))
        .property("speed", Property::builder(13).observed_at("2024-01-01T18:00:00Z".parse()?))
        .property("speed", Property::builder(14).observed_at("2024-01-01T19:00:00Z".parse()?))
        .property("speed", Property::builder(15).observed_at("2024-01-01T20:00:00Z".parse()?))
        .property("speed", Property::builder(16).observed_at("2024-01-01T21:00:00Z".parse()?))
        .property("speed", Property::builder(17).observed_at("2024-01-01T22:00:00Z".parse()?))
        .property("speed", Property::builder(18).observed_at("2024-01-01T23:00:00Z".parse()?))
        .property("speed", Property::builder(19).observed_at("2024-01-02T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-02T01:00:00Z".parse()?))
        .property("speed", Property::builder(21).observed_at("2024-01-02T02:00:00Z".parse()?))
        .property("speed", Property::builder(22).observed_at("2024-01-02T03:00:00Z".parse()?))
        .property("speed", Property::builder(23).observed_at("2024-01-02T04:00:00Z".parse()?))
        .property("speed", Property::builder(24).observed_at("2024-01-02T05:00:00Z".parse()?))
        .property("speed", Property::builder(25).observed_at("2024-01-02T06:00:00Z".parse()?))
        .property("speed", Property::builder(26).observed_at("2024-01-02T07:00:00Z".parse()?))
        .property("speed", Property::builder(27).observed_at("2024-01-02T08:00:00Z".parse()?))
        .property("speed", Property::builder(28).observed_at("2024-01-02T09:00:00Z".parse()?))
        .property("speed", Property::builder(29).observed_at("2024-01-02T10:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-02T11:00:00Z".parse()?))
        .property("speed", Property::builder(31).observed_at("2024-01-02T12:00:00Z".parse()?))
        .property("speed", Property::builder(32).observed_at("2024-01-02T13:00:00Z".parse()?))
        .property("speed", Property::builder(33).observed_at("2024-01-02T14:00:00Z".parse()?))
        .property("speed", Property::builder(34).observed_at("2024-01-02T15:00:00Z".parse()?))
        .property("speed", Property::builder(35).observed_at("2024-01-02T16:00:00Z".parse()?))
        .property("speed", Property::builder(36).observed_at("2024-01-02T17:00:00Z".parse()?))
        .property("speed", Property::builder(37).observed_at("2024-01-02T18:00:00Z".parse()?))
        .property("speed", Property::builder(38).observed_at("2024-01-02T19:00:00Z".parse()?))
        .property("speed", Property::builder(39).observed_at("2024-01-02T20:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_2_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_aggregated(
            temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-03T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum])
                .aggregation_period("PT1H".parse()?),
        )
        .entity_type(&agg_type)
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    let page = headed.outcome;
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T00:00:00Z".parse()?,
            "2024-01-01T19:00:00Z".parse()?,
            ContentRangeSize::Unknown
        )?))
    );
    expect_that!(page.items(), len(eq(2)));
    let aggregated = page.items().iter().find(|item| item.id == agg_id).or_fail()?;
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(len(eq(19)), nth(18, period().end("2024-01-01T19:00:00Z".parse()?)))
        )
    );
    let aggregated_2 = page.items().iter().find(|item| item.id == agg_2_id).or_fail()?;
    expect_that!(
        aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(14)),
                nth(0, period().start("2024-01-01T05:00:00Z".parse()?)),
                nth(13, period().end("2024-01-01T19:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
