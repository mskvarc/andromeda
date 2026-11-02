//! Every entity of a page answers its own history and its own aggregates.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities over three meters and an identifier no Entity has, with `lastN` 1,
/// answers each meter with its own latest instance, and with `aggrMethods=avg,max` answers each with the average
/// and maximum of its own instances; the identifier no Entity has answers nothing
/// Clause: 5.7.4.4, 4.5.19
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9383_every_entity_of_a_page_answers_its_own_history_and_aggregates() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_type = namespace.entity_type("PageMeter")?;
    let meters = [
        (namespace.urn("PageMeter", "low")?, [1, 2, 3]),
        (namespace.urn("PageMeter", "middle")?, [10, 20, 60]),
        (namespace.urn("PageMeter", "high")?, [100, 400, 100]),
    ];
    let absent_id = namespace.urn("PageMeter", "absent")?;
    for (id, values) in &meters {
        let evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(meter_type.clone())
            .property("power", Property::builder(values[0]).observed_at("2024-01-01T00:00:00Z".parse()?))
            .property("power", Property::builder(values[1]).observed_at("2024-01-01T00:15:00Z".parse()?))
            .property("power", Property::builder(values[2]).observed_at("2024-01-01T00:30:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let ids: Vec<_> = meters.iter().map(|(id, _)| id.clone()).chain([absent_id]).collect();
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let latest = json
        .temporal()
        .query_values(temporal_query::after("2023-12-31T00:00:00Z".parse()?).last_n(1))
        .entity_type(&meter_type)
        .ids(ids.clone())
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(latest.items(), len(eq(3)));
    for (id, values) in &meters {
        let meter = latest.items().iter().find(|item| &item.id == id).or_fail()?;
        expect_that!(
            meter,
            has_values("power", all!(len(eq(1)), nth(0, pair().value(values[2]).time("2024-01-01T00:30:00Z".parse()?))))
        );
    }
    let aggregated = json
        .temporal()
        .query_aggregated(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg, AggregationMethod::Max]))
        .entity_type(&meter_type)
        .ids(ids)
        .send()
        .await?;
    expect_that!(aggregated.items(), len(eq(3)));
    for ((id, _), (average, maximum)) in meters.iter().zip([(2, 3), (30, 60), (200, 400)]) {
        let meter = aggregated.items().iter().find(|item| &item.id == id).or_fail()?;
        expect_that!(
            meter,
            has_aggregate("power", AggregationMethod::Avg, all!(len(eq(1)), nth(0, period().value(average))))
        );
        expect_that!(
            meter,
            has_aggregate("power", AggregationMethod::Max, all!(len(eq(1)), nth(0, period().value(maximum))))
        );
    }
    Ok(())
}
