//! A registered source answering a malformed identity leaves the broker identity as it was.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ContextSourceAlias, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: After auxiliary registrations of sources whose identity lacks an id, has the wrong type, an empty alias,
/// an invalid uptime or an invalid `timeAt`, Retrieve Context Source Identity Information on the broker still
/// answers its own identity
/// Clause: 5.15.1.4, 5.2.40
/// Recorded: the specification defines the identity a source answers and says nothing of how a broker treats a
/// malformed one held by a registered source; Andromeda keeps answering its own identity
#[gtest]
#[tokio::test]
async fn o_inf_ret_6005_a_registered_source_answering_a_malformed_identity_leaves_the_broker_identity_as_it_was() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration_4_id = namespace.registration_id(4)?;
    let registration_5_id = namespace.registration_id(5)?;
    source.answer(Method::GET, "/1/ngsi-ld/v1/info/sourceIdentity", source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), "{\"type\": \"ContextSourceIdentity\", \"contextSourceAlias\": \"peer\", \"contextSourceUptime\": \"PT1H\", \"contextSourceTimeAt\": \"2026-01-01T00:00:00Z\"}")).await?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source.answer(Method::GET, "/2/ngsi-ld/v1/info/sourceIdentity", source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &format!("{{\"id\": \"urn:ngsi-ld:ContextSourceIdentity:{namespace}-peer\", \"type\": \"Vehicle_{namespace}\", \"contextSourceAlias\": \"peer\", \"contextSourceUptime\": \"PT1H\", \"contextSourceTimeAt\": \"2026-01-01T00:00:00Z\"}}"))).await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    source.answer(Method::GET, "/3/ngsi-ld/v1/info/sourceIdentity", source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &format!("{{\"id\": \"urn:ngsi-ld:ContextSourceIdentity:{namespace}-peer\", \"type\": \"ContextSourceIdentity\", \"contextSourceAlias\": \"\", \"contextSourceUptime\": \"PT1H\", \"contextSourceTimeAt\": \"2026-01-01T00:00:00Z\"}}"))).await?;
    let registration_3 = CSourceRegistration::builder(source.endpoint("3")?)
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    source.answer(Method::GET, "/4/ngsi-ld/v1/info/sourceIdentity", source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &format!("{{\"id\": \"urn:ngsi-ld:ContextSourceIdentity:{namespace}-peer\", \"type\": \"ContextSourceIdentity\", \"contextSourceAlias\": \"peer\", \"contextSourceUptime\": \"1 hour\", \"contextSourceTimeAt\": \"2026-01-01T00:00:00Z\"}}"))).await?;
    let registration_4 = CSourceRegistration::builder(source.endpoint("4")?)
        .id(registration_4_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_4).send().await?;
    let _registration_4_cleanup = Cleanup::of(&broker).registration(&registration_4_id);
    source.answer(Method::GET, "/5/ngsi-ld/v1/info/sourceIdentity", source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &format!("{{\"id\": \"urn:ngsi-ld:ContextSourceIdentity:{namespace}-peer\", \"type\": \"ContextSourceIdentity\", \"contextSourceAlias\": \"peer\", \"contextSourceUptime\": \"PT1H\", \"contextSourceTimeAt\": \"not-a-date\"}}"))).await?;
    let registration_5 = CSourceRegistration::builder(source.endpoint("5")?)
        .id(registration_5_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_5).send().await?;
    let _registration_5_cleanup = Cleanup::of(&broker).registration(&registration_5_id);
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity.alias, eq(&"broker-a".parse::<ContextSourceAlias>()?));
    Ok(())
}
