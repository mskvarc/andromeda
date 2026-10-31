//! A successful forward reports ok and one send.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    eventually::retried,
    namespace::Namespace,
    source_answer,
    teardown::AnyAnswer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode, RegistrationStatus},
};

/// Pins: After one Retrieve Entity forwarded to an exclusive registration whose source answers 200, the
/// registration reports status ok, `timesSent` 1 and a `lastSuccess`, and no `timesFailed` or `lastFailure`
/// Clause: Table 5.2.9-2, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_csr_hlt_9002_a_successful_forward_reports_ok_and_one_send() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_ok_id = namespace.urn("Vehicle", "ok")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_ok_id = namespace.registration_id("ok")?;
    let source_entity = Entity::output_builder()
        .id(vehicle_ok_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 42)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/ok/ngsi-ld/v1/entities/{vehicle_ok_id}"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let registration_ok = CSourceRegistration::builder(source.endpoint("ok")?)
        .id(registration_ok_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_ok_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_ok).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_ok_cleanup = Cleanup::of(&broker).registration(&registration_ok_id);
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_ok_id)
        .send()
        .await
        .any_answer();
    retried(async || {
        let registration = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_ok_id)
            .send()
            .await?;
        verify_that!(registration.output().status, some(eq(RegistrationStatus::Ok)))?;
        verify_that!(registration.output().times_sent, some(eq(1)))?;
        verify_that!(registration.output().status, some(anything()))?;
        verify_that!(registration.output().times_sent, some(anything()))?;
        verify_that!(registration.output().times_failed, none())?;
        verify_that!(registration.output().last_success, some(anything()))?;
        verify_that!(registration.output().last_failure, none())?;
        Ok(())
    })
    .await?;
    broker.registrations().delete(&registration_ok_id).send().await?;
    Ok(())
}
