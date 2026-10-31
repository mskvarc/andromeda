//! A temporal map by a type list asks only the registrations of the listed types.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with type Vehicle,Boat under inclusive registrations of Vehicle, of Boat and of Home advertising createEntityMapQueryTemporal answers 201 and asks the Vehicle and Boat sources once each and the Home one never
/// Clause: 5.14.5.4, 5.12, 4.17
/// Tenant: private (it counts every request its context sources receive, and the unscoped requests of other tests
/// reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qte_9312_a_temporal_map_by_a_type_list_asks_only_the_registrations_of_the_listed_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let boat_type = namespace.entity_type("Boat")?;
    let home_type = namespace.entity_type("Home")?;
    let registration_a = CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    broker.registrations().register(&registration_a).send().await?;
    let registration_b = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(boat_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    broker.registrations().register(&registration_b).send().await?;
    let registration_c = CSourceRegistration::builder(source.endpoint("c")?)
        .id(namespace.registration_id(3)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(home_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/b/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/c/ngsi-ld/v1/temporal/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    // The client writes a type selection with `|`, and the comma list is what the test sends, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entityMaps?type=Vehicle_{namespace}%2CBoat_{namespace}&timerel=before&timeAt=2022-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let asked_a = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source.received_matching(&SourceRequest::any_matching("/b/.*")).await?;
    expect_that!(asked_b, len(eq(1)));
    let asked_c = source.received_matching(&SourceRequest::any_matching("/c/.*")).await?;
    expect_that!(asked_c, len(eq(0)));
    Ok(())
}
