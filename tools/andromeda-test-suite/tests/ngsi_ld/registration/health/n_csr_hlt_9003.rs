//! A failed forward reports failed and one failure.

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
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode, RegistrationStatus},
};

/// Pins: After one Retrieve Entity forwarded to an exclusive registration whose source answers 502, the
/// registration reports status failed, `timesSent` 1, `timesFailed` 1 and a `lastFailure`, and no `lastSuccess`
/// Clause: Table 5.2.9-2, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_csr_hlt_9003_a_failed_forward_reports_failed_and_one_failure() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_failed_id = namespace.urn("Vehicle", "failed")?;
    let registration_failed_id = namespace.registration_id("failed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    source
        .answer(
            Method::GET,
            &format!("/failed/ngsi-ld/v1/entities/{vehicle_failed_id}"),
            source_answer::text(StatusCode::BAD_GATEWAY, HeaderValue::from_static("application/ld+json"), "{}"),
        )
        .await?;
    let registration_failed = CSourceRegistration::builder(source.endpoint("failed")?)
        .id(registration_failed_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_failed_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_failed).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_failed_cleanup = Cleanup::of(&broker).registration(&registration_failed_id);
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_failed_id)
        .send()
        .await
        .any_answer();
    retried(async || {
        let registration = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_failed_id)
            .send()
            .await?;
        verify_that!(registration.output().status, some(eq(RegistrationStatus::Failed)))?;
        verify_that!(registration.output().times_sent, some(eq(1)))?;
        verify_that!(registration.output().times_failed, some(eq(1)))?;
        verify_that!(registration.output().status, some(anything()))?;
        verify_that!(registration.output().times_sent, some(anything()))?;
        verify_that!(registration.output().times_failed, some(anything()))?;
        verify_that!(registration.output().last_success, none())?;
        verify_that!(registration.output().last_failure, some(anything()))?;
        Ok(())
    })
    .await?;
    broker.registrations().delete(&registration_failed_id).send().await?;
    Ok(())
}
