//! Instances of the parts are merged before they are aggregated.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        InstanceId,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with `splitEntities` true as `aggregatedValues` avg and `totalCount`
/// over two local speed instances and one source instance answers one aggregate over the three, and the source is
/// asked for instances rather than aggregates
/// Clause: 5.7.4.4, 4.5.19
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7429_instances_of_the_parts_are_merged_before_they_are_aggregated() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2020-08-01T13:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_evolutions = [TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .identified(
            "speed",
            format!("urn:ngsi-ld:instance:{namespace}-30").parse::<InstanceId>()?,
            Property::builder(30).observed_at("2020-08-01T14:00:00Z".parse()?),
        )
        .build()?];
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query_aggregated(temporal_query::before("2030-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg, AggregationMethod::TotalCount]))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(0))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let aggregated = page.items().first().or_fail()?;
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::TotalCount, nth(0, period().value(3))));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Avg, nth(0, period().value(20))));
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entities").await?;
    expect_that!(forwarded, len(ge(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "aggrMethods"), is_empty());
    expect_that!(query_values(forwarded_one, "format"), is_empty());
    Ok(())
}
