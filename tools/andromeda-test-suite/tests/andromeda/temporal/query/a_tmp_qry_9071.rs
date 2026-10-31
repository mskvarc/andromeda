//! An aggregation over respond async updates is answered at once.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchUpdate},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, evolution::AggregationMethod},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution with `aggregatedValues`, `aggrMethods` sum and max sent straight after two Batch
/// Entity Updates answered 204 with Preference-Applied: respond-async wrote speed 5 and 3 over a created speed 1,
/// with no retry, answers the sum 9 and the maximum 5 of the three
/// Clause: 5.6.9, 5.7.4, 4.5.19
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), whose history is queued with the write and answered by every temporal read before the queue is applied;
/// clause 4.5.19 aggregates the Attribute instances of the window, and the instances the writes recorded are among
/// them from the moment each write is acknowledged
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9071_an_aggregation_over_respond_async_updates_is_answered_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car = Entity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(1).observed_at("2026-03-01T10:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&car).send().await?.done()?;
    for (speed, observed_at) in [(5, "2026-03-01T10:01:00Z"), (3, "2026-03-01T10:02:00Z")] {
        let fragments = [EntityFragment::for_entity(car_id.clone(), car_type.clone())
            .property("speed", Property::builder(speed).observed_at(observed_at.parse()?))
            .build()?];
        let request = respond_async(broker.unchecked(&BatchUpdate::builder().fragments(&fragments).build())?);
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, async_applied(StatusCode::NO_CONTENT));
    }
    // Asked at once, with no retry.
    let aggregated = broker
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
