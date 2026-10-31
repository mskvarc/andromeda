//! A failed forward sets the status failed and counts the failure.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    eventually::retried,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
    teardown::AnyAnswer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode, RegistrationStatus},
};

/// Pins: After Retrieve Entity is forwarded once to an exclusive registration whose source answers 502, the
/// registration reads back with status failed, `timesSent` 1, `timesFailed` 1, a `lastFailure` and no `lastSuccess`
/// Clause: 5.2.9, 5.7.1
#[gtest]
#[tokio::test]
async fn n_csr_hlt_6002_a_failed_forward_sets_the_status_failed_and_counts_the_failure() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).priority(1),
            source_answer::status(StatusCode::BAD_GATEWAY),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await
        .any_answer();
    retried(async || {
        let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
        verify_that!(registration_2.output().status, some(eq(RegistrationStatus::Failed)))?;
        verify_that!(registration_2.output().times_sent, some(eq(1)))?;
        verify_that!(registration_2.output().times_failed, some(eq(1)))?;
        verify_that!(registration_2.output().last_failure, some(anything()))?;
        verify_that!(registration_2.output().last_success, none())?;
        Ok(())
    })
    .await?;
    Ok(())
}
