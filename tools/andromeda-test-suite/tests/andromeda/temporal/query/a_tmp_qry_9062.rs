//! An aggregation is answered on a deferred broker.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    config::BrokerRole,
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

/// Pins: on a broker that defers its history, Query Temporal Evolution with aggregatedValues, aggrMethods sum and max
/// over the window sent right after three speed observations written by three requests answers the sum 9 and the
/// maximum 5 of the three
/// Clause: 5.6.11, 5.6.12, 5.7.4, 4.5.19
/// Recorded: the deferred temporal append mode is Andromeda's; clause 4.5.19 aggregates the Attribute instances of
/// the window, and the instances the writes recorded are among them from the moment each write is acknowledged
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9062_an_aggregation_is_answered_on_a_deferred_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car = TemporalEntity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(1).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?;
    broker_not_recorded.temporal().create_or_update(&car).send().await?.created()?;
    for (speed, observed_at) in [(5, "2026-03-01T10:01:00Z"), (3, "2026-03-01T10:02:00Z")] {
        let instances = TemporalEntity::builder()
            .id(car_id.clone())
            .entity_type(car_type.clone())
            .property("speed", Property::builder(speed).observed_at(observed_at.parse()?))
            .build()?
            .attributes;
        broker_not_recorded.temporal().add_attributes(&car_id, &instances).send().await?;
    }
    let aggregated = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_aggregated(temporal_query::after("2026-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum, AggregationMethod::Max]))
        .entity_type(&car_type)
        .send()
        .await?;
    expect_that!(aggregated.items(), len(eq(1)));
    let car_aggregate = aggregated.items().first().or_fail()?;
    expect_that!(car_aggregate, has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(9))));
    expect_that!(car_aggregate, has_aggregate("speed", AggregationMethod::Max, nth(0, period().value(5))));
    Ok(())
}
