//! A transient version gives its expiry to its own attributes.

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
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity merging a version with `expiresAt` 2099-01-01 and speed and a version without `expiresAt`
/// holding `fuelLevel` answers speed with `expiresAt` 2099-01-01, `fuelLevel` without `expiresAt`, and no
/// entity-level `expiresAt`
/// Clause: 4.5.5.2, 4.5.5.3, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_9010_a_transient_version_gives_its_expiry_to_its_own_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let transient_probe_push_down_id = namespace.urn("TransientProbe", "push-down")?;
    let transient_probe_type = namespace.entity_type("TransientProbe")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let source_entity = Entity::output_builder()
        .id(transient_probe_push_down_id.clone())
        .entity_type(transient_probe_type.clone())
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("speed", 60)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{transient_probe_push_down_id}"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type.clone()).with_id(transient_probe_push_down_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entity_2 = Entity::output_builder()
        .id(transient_probe_push_down_id.clone())
        .entity_type(transient_probe_type.clone())
        .property("fuelLevel", 70)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/b/ngsi-ld/v1/entities/{transient_probe_push_down_id}"),
            source_answer::entity(&source_entity_2),
        )
        .await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type.clone()).with_id(transient_probe_push_down_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&transient_probe_push_down_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.expires_at, none());
    expect_that!(entity, has_attribute("speed", any_attribute().expires_at("2099-01-01T00:00:00Z".parse()?)));
    expect_that!(entity, has_attribute("fuelLevel", any_attribute().value(70).without(Member::ExpiresAt)));
    broker.registrations().delete(&registration_id).send().await?;
    broker.registrations().delete(&registration_2_id).send().await?;
    Ok(())
}
