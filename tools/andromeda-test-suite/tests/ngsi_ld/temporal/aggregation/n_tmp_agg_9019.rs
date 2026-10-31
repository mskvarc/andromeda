//! A whole range aggregation is one period however many instances it holds.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    header_matcher::absent,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentRange},
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: 40 hourly instances aggregated without `aggrPeriodDuration` between 2024-01-01 and 2024-01-03 answer 200
/// with one period from 2024-01-01T00:00:00Z to 2024-01-03T00:00:00Z counting 40, more instances than the base
/// stack's page
/// Clause: 4.5.19, 4.5.19.1, 6.3.10
/// Recorded: an absent duration spans the whole time range of the query, one period, which is never split across
/// pages
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9019_a_whole_range_aggregation_is_one_period_however_many_instances_it_holds() -> Result<()> {
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
                .aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum]),
        )
        .send()
        .await?;
    expect_that!(headed.head.typed::<ContentRange>(), absent());
    let retrieved = &headed.outcome;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(1)),
                nth(
                    0,
                    period().value(40).start("2024-01-01T00:00:00Z".parse()?).end("2024-01-03T00:00:00Z".parse()?)
                )
            )
        )
    );
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(780))));
    Ok(())
}
