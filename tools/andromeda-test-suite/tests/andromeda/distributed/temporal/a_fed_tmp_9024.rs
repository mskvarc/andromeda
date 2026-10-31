//! A whole range period is one range for local and merged aggregates.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::{EntitySplitting, IdPattern, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with splitEntities true as aggregatedValues totalCount, timerel after and a q over one Vehicle only the broker holds (latest instance 10:00) and one held on both sides (latest instance 11:00) reports the period 2020-01-01T00:00:00Z to 2020-08-01T11:00:00Z on both
/// Clause: 4.5.19.1, 6.3.10, 5.7.4.4
/// Recorded: the aggregates the broker computes for one response, over its own instances and over the merged instances of a contributed Entity, are closed to the widest instant of them all
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9024_a_whole_range_period_is_one_range_for_local_and_merged_aggregates() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T09:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2020-08-01T10:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_2_evolution = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2020-08-01T09:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_2_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:Vehicle:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The source answers an instance that carries an instanceId, which the model builds none of, so its answer is text.
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities"),
            source_answer::text(
                StatusCode::OK,
                HeaderValue::from_static("application/json"),
                &format!(
                    r#"[{{"id": "urn:ngsi-ld:Vehicle:{namespace}-2", "type": "Vehicle_{namespace}", "speed": [{{"type": "Property", "value": 40, "observedAt": "2020-08-01T11:00:00Z", "instanceId": "urn:ngsi-ld:instance:{namespace}-40"}}]}}]"#
                ),
            ),
        )
        .await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_aggregated(temporal_query::after("2020-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::TotalCount]))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(0))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    for id in [&vehicle_id, &vehicle_2_id] {
        let aggregated = page.items().iter().find(|item| &item.id == id).or_fail()?;
        expect_that!(
            aggregated,
            has_aggregate(
                "speed",
                AggregationMethod::TotalCount,
                all!(
                    len(eq(1)),
                    nth(0, period().value(2).start("2020-01-01T00:00:00Z".parse()?).end("2020-08-01T11:00:00Z".parse()?))
                )
            )
        );
    }
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
