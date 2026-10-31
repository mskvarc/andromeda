//! An aggregation pages by whole periods and resumes on their grid.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    header_matcher::absent,
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

/// Pins: 40 hourly instances aggregated per PT1H between 2024-01-01 and 2024-01-03 answer 206 with the first 19
/// periods and Content-Range `DateTime` 2024-01-01T00:00:00Z-2024-01-01T19:00:00Z/*; resumed at 19:00 they answer
/// 206 with the next 19, to 2024-01-02T14:00:00Z; resumed again they answer 200 with the last 2, every period
/// counting one instance
/// Clause: 4.5.19, 6.3.10
/// Recorded: Andromeda configuration, not v1.9.1: a period is the element of the aggregated representation, and the
/// base stack answers at most 19 of them per attribute (clause 6.3.10 leaves the limit to the implementation); a
/// page ends at the end of the last period it serves
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9018_an_aggregation_pages_by_whole_periods_and_resumes_on_their_grid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
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
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-03T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum])
                .aggregation_period("PT1H".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T00:00:00Z".parse()?,
            "2024-01-01T19:00:00Z".parse()?,
            ContentRangeSize::Unknown
        )?))
    );
    let retrieved = &headed.outcome;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(19)),
                nth(0, period().value(1).start("2024-01-01T00:00:00Z".parse()?).end("2024-01-01T01:00:00Z".parse()?)),
                nth(
                    18,
                    period().value(1).start("2024-01-01T18:00:00Z".parse()?).end("2024-01-01T19:00:00Z".parse()?)
                )
            )
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                len(eq(19)),
                nth(
                    18,
                    period().value(18).start("2024-01-01T18:00:00Z".parse()?).end("2024-01-01T19:00:00Z".parse()?)
                )
            )
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2024-01-01T19:00:00Z".parse()?, "2024-01-03T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum])
                .aggregation_period("PT1H".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed_2.status, eq(StatusCode::PARTIAL_CONTENT));
    expect_that!(
        headed_2.head.typed::<ContentRange>().valid(),
        some(eq(&ContentRange::try_new(
            "2024-01-01T19:00:00Z".parse()?,
            "2024-01-02T14:00:00Z".parse()?,
            ContentRangeSize::Unknown
        )?))
    );
    let retrieved_2 = &headed_2.outcome;
    let aggregated_2 = retrieved_2.body();
    expect_that!(aggregated_2, has_aggregate("speed", AggregationMethod::TotalCount, len(eq(19))));
    expect_that!(
        aggregated_2,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                nth(
                    0,
                    period().value(19).start("2024-01-01T19:00:00Z".parse()?).end("2024-01-01T20:00:00Z".parse()?)
                ),
                nth(
                    18,
                    period().value(37).start("2024-01-02T13:00:00Z".parse()?).end("2024-01-02T14:00:00Z".parse()?)
                )
            )
        )
    );
    let headed_3 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2024-01-02T14:00:00Z".parse()?, "2024-01-03T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum])
                .aggregation_period("PT1H".parse()?),
        )
        .send()
        .await?;
    expect_that!(headed_3.head.typed::<ContentRange>(), absent());
    let retrieved_3 = &headed_3.outcome;
    let aggregated_3 = retrieved_3.body();
    expect_that!(
        aggregated_3,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(2)),
                nth(0, period().value(1).start("2024-01-02T14:00:00Z".parse()?).end("2024-01-02T15:00:00Z".parse()?))
            )
        )
    );
    expect_that!(
        aggregated_3,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                nth(
                    0,
                    period().value(38).start("2024-01-02T14:00:00Z".parse()?).end("2024-01-02T15:00:00Z".parse()?)
                ),
                nth(
                    1,
                    period().value(39).start("2024-01-02T15:00:00Z".parse()?).end("2024-01-02T16:00:00Z".parse()?)
                )
            )
        )
    );
    Ok(())
}
