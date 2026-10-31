//! Purge by a type conjunction is forwarded only to a registration listing both types.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer, source_request::SourceRequest};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::OneOrMany,
    query::type_selection,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities with type (Home;Vehicle) under redirect registrations advertising purgeEntity, one listing both Home and Vehicle for its entities, one Vehicle alone and one Home alone, answers 204 and forwards a purge to the first source only
/// Clause: 5.6.21.4, 5.12, 4.17, 5.2.8
/// Tenant: private (it counts every request its context sources receive, and a purge forwarded to every
/// registration of the tenant reaches the sources of other tests)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9313_purge_by_a_type_conjunction_is_forwarded_only_to_a_registration_listing_both_types() -> Result<()> {
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
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_a).send().await?;
    let registration_b = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
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
    broker
        .entities()
        .purge()
        .entity_type(type_selection::all_of([home_type.as_str(), vehicle_type.as_str()]).build()?)
        .send()
        .await?
        .done()?;
    let asked_a = source
        .received_matching(&SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source
        .received_matching(&SourceRequest::any_matching("/b/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_b, len(eq(0)));
    let asked_c = source
        .received_matching(&SourceRequest::any_matching("/c/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_c, len(eq(0)));
    Ok(())
}
