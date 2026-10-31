//! A query reaches only query temporal sources and local reaches none.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, source_answer, source_request::SourceRequest};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    query::{Locality, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities by type is forwarded once to the registration advertising
/// `queryTemporal` and never to the one advertising only `retrieveTemporal`, and with local=true to neither
/// Clause: 5.7.4.4, 4.20, 5.5.13
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7407_a_query_reaches_only_query_temporal_sources_and_local_reaches_none() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("qt")?)
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
    let registration_2 = CSourceRegistration::builder(source.endpoint("rt")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let source_evolutions = [];
    source
        .answer(
            Method::GET,
            "/qt/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let source_evolutions_2 = [];
    source
        .answer(
            Method::GET,
            "/rt/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&source_evolutions_2),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let forwarded_request = SourceRequest::matching(Method::GET, "/qt/ngsi-ld/v1/temporal/entities");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::matching(Method::GET, "/rt/.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let forwarded_request_3 = SourceRequest::matching(Method::GET, "/qt/ngsi-ld/v1/temporal/entities");
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(1)));
    let forwarded_request_4 = SourceRequest::matching(Method::GET, "/rt/.*");
    let forwarded_4 = source.received_matching(&forwarded_request_4).await?;
    expect_that!(forwarded_4, len(eq(0)));
    Ok(())
}
