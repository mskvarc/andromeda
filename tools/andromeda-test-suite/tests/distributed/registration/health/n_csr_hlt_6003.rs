//! The status follows the last forward and the counters count every attempt.

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
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode, RegistrationStatus},
};

/// Pins: After forwarded retrievals that succeed, fail with 502 and succeed again, the registration reads back with
/// status ok after the first, failed after the second and ok after the third, `timesSent` counting all three,
/// `timesFailed` the one failure, and `lastSuccess` and `lastFailure` set
/// Clause: 5.2.9, 5.7.1
#[gtest]
#[tokio::test]
async fn n_csr_hlt_6003_the_status_follows_the_last_forward_and_the_counters_count_every_attempt() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let source_entity = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).priority(1),
            source_answer::entity(&source_entity),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, &format!("/a/ngsi-ld/v1/entities/{vehicle_2_id}")).priority(1),
            source_answer::status(StatusCode::BAD_GATEWAY),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_id));
    retried(async || {
        let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
        verify_that!(registration_2.output().status, some(eq(RegistrationStatus::Ok)))?;
        verify_that!(registration_2.output().times_sent, some(eq(1)))?;
        verify_that!(registration_2.output().last_success, some(anything()))?;
        Ok(())
    })
    .await?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .send()
        .await
        .any_answer();
    retried(async || {
        let registration_3 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
        verify_that!(registration_3.output().status, some(eq(RegistrationStatus::Failed)))?;
        verify_that!(registration_3.output().times_sent, some(eq(2)))?;
        verify_that!(registration_3.output().times_failed, some(eq(1)))?;
        verify_that!(registration_3.output().last_success, some(anything()))?;
        verify_that!(registration_3.output().last_failure, some(anything()))?;
        Ok(())
    })
    .await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&vehicle_id));
    retried(async || {
        let registration_4 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
        verify_that!(registration_4.output().status, some(eq(RegistrationStatus::Ok)))?;
        verify_that!(registration_4.output().times_sent, some(eq(3)))?;
        verify_that!(registration_4.output().times_failed, some(eq(1)))?;
        Ok(())
    })
    .await?;
    Ok(())
}
