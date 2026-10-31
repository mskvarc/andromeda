//! Aggregation counts the latest and the replaced instances once.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, evolution::AggregationMethod},
    query::temporal_query,
};

/// Pins: of a speed a Core write set to 10, 20 and then 30, Retrieve Temporal Evolution of an Entity and Query
/// Temporal Evolution of Entities with `aggrMethods` totalCount,sum,max over a window holding the three answer
/// `totalCount` 3, sum 60 and max 30, and over a window holding the latest alone `totalCount` 1 and sum 30
/// Clause: 4.3.1, 5.7.3.4, 5.7.4.4, Table 4.5.19.1-1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9038_aggregation_counts_the_latest_and_the_replaced_instances_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_counted_id = namespace.urn("Vehicle", "counted")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_counted = Entity::builder()
        .id(vehicle_counted_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_counted).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(20).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_counted_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder()
        .property("speed", Property::builder(30).observed_at("2024-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_counted_id, &fragment_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&vehicle_counted_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([
            AggregationMethod::TotalCount,
            AggregationMethod::Sum,
            AggregationMethod::Max,
        ]))
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::TotalCount, nth(0, period().value(3))));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(60))));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Max, nth(0, period().value(30))));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_aggregated(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum]))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let aggregated_2 = page.items().first().or_fail()?;
    expect_that!(aggregated_2, has_aggregate("speed", AggregationMethod::TotalCount, nth(0, period().value(3))));
    expect_that!(aggregated_2, has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(60))));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&vehicle_counted_id)
        .temporal_q(temporal_query::after("2024-01-02T12:00:00Z".parse()?).aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum]))
        .send()
        .await?;
    let aggregated_3 = retrieved_2.body();
    expect_that!(aggregated_3, has_aggregate("speed", AggregationMethod::TotalCount, nth(0, period().value(1))));
    expect_that!(aggregated_3, has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(30))));
    Ok(())
}
