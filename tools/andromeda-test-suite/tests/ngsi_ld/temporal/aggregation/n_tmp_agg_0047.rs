//! A period holding no instance has no triple.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: avg with `aggrPeriodDuration` PT1H over a between query from 00:00 to 03:00, with instances only in the
/// first and the third hour, answers two triples, 00:00 to 01:00 and 02:00 to 03:00, and none for the hour holding
/// no instance
/// Clause: 4.5.19, 4.5.19.1, 4.5.0, 5.7.3.4
/// Recorded: 4.5.19 asks for "as many array elements as there are periods in the time range of the query", each
/// holding "the value obtained after applying the aggregation method over the period"; avg over a period holding no
/// instance obtains no value, and 4.5.0 rules out null ("null cannot be used as a value in NGSI-LD"), so such a
/// period has no triple, as in the example of annex C, clause C.5.14, which answers two of the fifteen periods of
/// its range. Ruled by the user (2026-10-11)
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0047_a_period_holding_no_instance_has_no_triple() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:10:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T02:10:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2024-01-01T00:00:00Z".parse()?, "2024-01-01T03:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Avg])
                .aggregation_period("PT1H".parse()?),
        )
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Avg,
            all!(
                len(eq(2)),
                nth(
                    0,
                    period().value(10).start("2024-01-01T00:00:00Z".parse()?).end("2024-01-01T01:00:00Z".parse()?)
                ),
                nth(
                    1,
                    period().value(20).start("2024-01-01T02:00:00Z".parse()?).end("2024-01-01T03:00:00Z".parse()?)
                )
            )
        )
    );
    Ok(())
}
