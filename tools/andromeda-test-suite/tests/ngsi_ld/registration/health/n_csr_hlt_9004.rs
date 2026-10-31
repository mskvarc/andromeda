//! A success after a failure reports ok and keeps the failure.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    eventually::retried,
    namespace::Namespace,
    source_answer,
    teardown::AnyAnswer,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode, RegistrationStatus},
};
use wiremock::{
    Mock,
    matchers::{method, path},
};

/// Pins: A forwarded Retrieve Entity answered 502 and then one answered 200 leave the registration at status ok,
/// `timesSent` 2, `timesFailed` 1, with both `lastFailure` and `lastSuccess`
/// Clause: Table 5.2.9-2, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_csr_hlt_9004_a_success_after_a_failure_reports_ok_and_keeps_the_failure() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", "recovery")?;
    let registration_id = namespace.registration_id("recovery")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let served = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(42))
        .build()?;
    source
        .answer_in_turn(
            || Mock::given(method("GET")).and(path(format!("/recovery/ngsi-ld/v1/entities/{vehicle_id}"))),
            vec![
                source_answer::text(StatusCode::BAD_GATEWAY, HeaderValue::from_static("application/ld+json"), "{}"),
                source_answer::entity(&served),
            ],
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("recovery")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await
        .any_answer();
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_id)
            .send()
            .await?;
        verify_that!(retrieved.output().status, some(eq(RegistrationStatus::Failed)))?;
        Ok(())
    })
    .await?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .send()
            .await?;
        verify_that!(retrieved.body(), has_attribute("speed", property().value(42)))?;
        Ok(())
    })
    .await?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_id)
            .send()
            .await?;
        verify_that!(retrieved.output().status, some(eq(RegistrationStatus::Ok)))?;
        verify_that!(retrieved.output().times_sent, some(eq(2)))?;
        verify_that!(retrieved.output().times_failed, some(eq(1)))?;
        verify_that!(retrieved.output().last_success, some(anything()))?;
        verify_that!(retrieved.output().last_failure, some(anything()))?;
        Ok(())
    })
    .await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
