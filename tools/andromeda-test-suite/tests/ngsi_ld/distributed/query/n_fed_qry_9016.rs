//! The locally held version is a party to the conflict.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity of a locally held Entity with `expiresAt` 2099-01-01 and speed, merged with a source
/// version without `expiresAt` holding `fuelLevel`, answers no entity-level `expiresAt`, speed with `expiresAt`
/// 2099-01-01 and `fuelLevel` without one
/// Clause: 4.5.5.2, 4.5.5.3, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_9016_the_locally_held_version_is_a_party_to_the_conflict() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let transient_probe_local_party_id = namespace.urn("TransientProbe", "local-party")?;
    let transient_probe_type = namespace.entity_type("TransientProbe")?;
    let registration_id = namespace.registration_id(1)?;
    let transient_probe_local_party = Entity::builder()
        .id(transient_probe_local_party_id.clone())
        .entity_type(transient_probe_type.clone())
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("speed", 60)
        .build()?;
    broker.entities().create(&transient_probe_local_party).send().await?.done()?;
    let source_entity = Entity::output_builder()
        .id(transient_probe_local_party_id.clone())
        .entity_type(transient_probe_type.clone())
        .property("fuelLevel", 70)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{transient_probe_local_party_id}"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type.clone()).with_id(transient_probe_local_party_id.clone())])
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
        .retrieve(&transient_probe_local_party_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.expires_at, none());
    expect_that!(entity, has_attribute("speed", any_attribute().expires_at("2099-01-01T00:00:00Z".parse()?)));
    expect_that!(entity, has_attribute("fuelLevel", any_attribute().value(70).without(Member::ExpiresAt)));
    broker.registrations().delete(&registration_id).send().await?;
    broker.entities().delete(&transient_probe_local_party_id).send().await?.done()?;
    Ok(())
}
