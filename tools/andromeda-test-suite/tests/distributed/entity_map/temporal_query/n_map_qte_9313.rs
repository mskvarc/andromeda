//! A temporal map asks only the registrations its context source filter admits.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    context_source_property::property_value,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities by type with csf tier=="gold" under two
/// inclusive registrations advertising `createEntityMapQueryTemporal`, tier gold and tier silver, answers 201 and
/// asks the gold source only
/// Clause: 5.14.5.3, 5.14.5.4, 4.9
/// Tenant: private (it counts every request its context sources receive, and the unscoped requests of other tests
/// reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qte_9313_a_temporal_map_asks_only_the_registrations_its_context_source_filter_admits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .property("tier", property_value("gold"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .property("tier", property_value("silver"))
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/b/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .csf(q::attr("tier").eq("gold"))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let asked_a = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source.received_matching(&SourceRequest::any_matching("/b/.*")).await?;
    expect_that!(asked_b, len(eq(0)));
    Ok(())
}
