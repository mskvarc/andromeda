//! Versions that agree keep the entity level expiry.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity merging two versions both with `expiresAt` 2099-01-01 answers the entity-level `expiresAt`
/// 2099-01-01, and speed and `fuelLevel` without one
/// Clause: 4.5.5.2, 4.5.5.3, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_9014_versions_that_agree_keep_the_entity_level_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let transient_probe_agreed_id = namespace.urn("TransientProbe", "agreed")?;
    let transient_probe_type = namespace.entity_type("TransientProbe")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let source_entity = Entity::output_builder()
        .id(transient_probe_agreed_id.clone())
        .entity_type(transient_probe_type.clone())
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("speed", 60)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{transient_probe_agreed_id}"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type.clone()).with_id(transient_probe_agreed_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entity_2 = Entity::output_builder()
        .id(transient_probe_agreed_id.clone())
        .entity_type(transient_probe_type.clone())
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("fuelLevel", 70)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/b/ngsi-ld/v1/entities/{transient_probe_agreed_id}"),
            source_answer::entity(&source_entity_2),
        )
        .await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type.clone()).with_id(transient_probe_agreed_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&transient_probe_agreed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.expires_at, some(eq("2099-01-01T00:00:00Z".parse::<DateTime>()?)));
    expect_that!(entity, has_attribute("speed", any_attribute().without(Member::ExpiresAt)));
    expect_that!(entity, has_attribute("fuelLevel", any_attribute().without(Member::ExpiresAt)));
    broker.registrations().delete(&registration_id).send().await?;
    broker.registrations().delete(&registration_2_id).send().await?;
    Ok(())
}
