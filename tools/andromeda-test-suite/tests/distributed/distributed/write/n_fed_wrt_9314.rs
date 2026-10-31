//! Purge is forwarded only to the registrations its context source filter admits.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    context_source_property::property_value,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities by type with csf tier=="gold" under two redirect registrations advertising `purgeEntity`,
/// tier gold and tier silver, answers 204 and forwards a purge to the gold source only
/// Clause: 5.6.21.3, 5.6.21.4, 4.9
/// Tenant: private (it counts every request its context sources receive, and a purge forwarded to every
/// registration of the tenant reaches the sources of other tests)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9314_purge_is_forwarded_only_to_the_registrations_its_context_source_filter_admits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .property("tier", property_value("gold"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
        .property("tier", property_value("silver"))
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    source
        .answer(Method::DELETE, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    source
        .answer(Method::DELETE, "/b/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    broker
        .entities()
        .purge()
        .entity_type(&vehicle_type)
        .csf(q::attr("tier").eq("gold"))
        .send()
        .await?
        .done()?;
    let forwarded = source
        .received_matching(&SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_2 = source
        .received_matching(&SourceRequest::any_matching("/b/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(forwarded_2, len(eq(0)));
    Ok(())
}
