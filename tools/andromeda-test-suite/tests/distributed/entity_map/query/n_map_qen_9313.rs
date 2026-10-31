//! A map by a type conjunction asks only a registration listing both types.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany},
    query::type_selection,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Entities with type (Home;Vehicle) under inclusive registrations advertising createEntityMapQueryEntity, one listing both Home and Vehicle for its entities, one Vehicle alone and one Home alone, answers 201 and asks the first source only
/// Clause: 5.14.4.4, 5.12, 4.17, 5.2.8
/// Tenant: private (it counts every request its context sources receive, and the unscoped requests of other tests
/// reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qen_9313_a_map_by_a_type_conjunction_asks_only_a_registration_listing_both_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let home_type = namespace.entity_type("Home")?;
    let registration_a = CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(OneOrMany::try_from(vec![home_type.clone(), vehicle_type.clone()])?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration_a).send().await?;
    let registration_b = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration_b).send().await?;
    let registration_c = CSourceRegistration::builder(source.endpoint("c")?)
        .id(namespace.registration_id(3)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(home_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/b/ngsi-ld/v1/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    source
        .answer(Method::GET, "/c/ngsi-ld/v1/entityMaps", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(type_selection::all_of([home_type.as_str(), vehicle_type.as_str()]).build()?)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let asked_a = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source.received_matching(&SourceRequest::any_matching("/b/.*")).await?;
    expect_that!(asked_b, len(eq(0)));
    let asked_c = source.received_matching(&SourceRequest::any_matching("/c/.*")).await?;
    expect_that!(asked_c, len(eq(0)));
    Ok(())
}
