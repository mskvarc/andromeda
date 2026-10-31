//! A forwarded temporal query carries the temporal parameters and a via header.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_head,
    forwarded_query::query_values,
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with timerel after, `timeAt` 2024-01-01T00:00:00Z and `lastN` 5 is
/// forwarded once to the matching source carrying the same timerel, `timeAt` and `lastN` and a Via header
/// Clause: 5.7.4.4, 6.3.18
#[gtest]
#[tokio::test]
async fn n_fed_tmp_0008_a_forwarded_temporal_query_carries_the_temporal_parameters_and_a_via_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
            .build()?])
        .operations([
            DistributedOperation::RetrieveEntity,
            DistributedOperation::QueryEntity,
            DistributedOperation::RetrieveTemporal,
            DistributedOperation::QueryTemporal,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_evolutions = [];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::after("2024-01-01T00:00:00Z".parse()?).last_n(5))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "timerel"), nth(0, eq("after")));
    expect_that!(query_values(forwarded_one, "timeAt"), nth(0, starts_with("2024-01-01T00:00:00")));
    expect_that!(query_values(forwarded_one, "lastN"), nth(0, eq("5")));
    expect_that!(forwarded_head::via_hosts(forwarded_one), some(anything()));
    Ok(())
}
