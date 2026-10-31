//! Redirect temporal upsert forwards the whole evolution and creates nothing locally.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    forwarded_body,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Temporal Evolution with `brandName` and speed under a redirect registration advertising
/// `upsertTemporal` answers a success, forwards one create carrying both attributes, and creates nothing locally
/// Clause: 5.6.11.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_tmp_6212_redirect_temporal_upsert_forwards_the_whole_evolution_and_creates_nothing_locally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::UpsertTemporal, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/temporal/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("Mercedes").observed_at("2021-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(42).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    let answer = broker.with_responses().temporal().create_or_update(&vehicle_evolution).send().await;
    expect_that!(answer.answered_status().map(|status| status.as_u16()), some(all!(ge(200), lt(205))));
    let forwarded = source.received_on(Method::POST, "/a/ngsi-ld/v1/temporal/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_evolution = forwarded_body::evolution(forwarded_one)?;
    expect_that!(&forwarded_evolution, has_evolution("speed", nth(0, any_attribute().value(42))));
    expect_that!(&forwarded_evolution, has_evolution("brandName", nth(0, any_attribute().value("Mercedes"))));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
