//! Purge by a type list is forwarded only to the registrations of the listed types.

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
use http::{Method, StatusCode};
use ngsi_ld::{
    self,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities with type Vehicle,Boat under redirect registrations of Vehicle and of Boat advertising purgeEntity and of Home advertising only retrieveEntity answers 204 and forwards one purge to each of the Vehicle and Boat sources and nothing to the Home one
/// Clause: 5.6.21.4, 5.12, 4.17
/// Tenant: private (it counts every request its context sources receive, and a purge forwarded to every
/// registration of the tenant reaches the sources of other tests)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9312_purge_by_a_type_list_is_forwarded_only_to_the_registrations_of_the_listed_types() -> Result<()> {
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
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_a).send().await?;
    let registration_b = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(boat_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_b).send().await?;
    let registration_c = CSourceRegistration::builder(source.endpoint("c")?)
        .id(namespace.registration_id(3)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(home_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    source
        .answer(Method::DELETE, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    source
        .answer(Method::DELETE, "/b/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    source
        .answer(Method::DELETE, "/c/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    // The client writes a type selection with `|`, and the comma list is what the test sends, so the query is sent as written.
    let request = request_to(&broker, Method::DELETE, &format!("entities?type=Vehicle_{namespace}%2CBoat_{namespace}"))?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let asked_a = source
        .received_matching(&SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source
        .received_matching(&SourceRequest::any_matching("/b/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_b, len(eq(1)));
    let asked_c = source
        .received_matching(&SourceRequest::any_matching("/c/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_c, len(eq(0)));
    Ok(())
}
