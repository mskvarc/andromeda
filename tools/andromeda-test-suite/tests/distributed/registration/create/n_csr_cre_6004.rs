//! A registration is deleted once its expires at is reached.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, eventually::retried, namespace::Namespace, problem::problem};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, ErrorType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with an `expiresAt` a few seconds ahead answers 201 and the
/// registration reads back, then reads 404 `ResourceNotFound` once that instant has passed
/// Clause: 5.9.2.4
#[gtest]
#[tokio::test]
async fn n_csr_cre_6004_a_registration_is_deleted_once_its_expires_at_is_reached() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let in_5_seconds: DateTime = (Utc::now() + TimeDelta::seconds(5)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .expires_at(in_5_seconds)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_2.expires_at().map(ToString::to_string), some(contains_regex("^20[0-9][0-9]-")));
    retried(async || {
        let answer = broker
            .with_responses()
            .with_accept(Accept::any())
            .registrations()
            .retrieve(&registration_id)
            .send()
            .await;
        verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound))?;
        Ok(())
    })
    .await?;
    Ok(())
}
