//! The query merges transient versions alike.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities merging a version with `expiresAt` 2099-01-01 and speed and a version without `expiresAt`
/// holding `fuelLevel` answers speed with `expiresAt` 2099-01-01, `fuelLevel` without one, and no entity-level
/// `expiresAt`
/// Clause: 4.5.5.2, 4.5.5.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_9018_the_query_merges_transient_versions_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let transient_probe_query_id = namespace.urn("TransientProbe", "query")?;
    let transient_probe_type = namespace.entity_type("TransientProbe")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let source_entities = [Entity::output_builder()
        .id(transient_probe_query_id.clone())
        .entity_type(transient_probe_type.clone())
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("speed", 60)
        .build()?];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [Entity::output_builder()
        .id(transient_probe_query_id.clone())
        .entity_type(transient_probe_type.clone())
        .property("fuelLevel", 70)
        .build()?];
    source
        .answer(Method::GET, "/b/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&transient_probe_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(item.expires_at, none());
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&transient_probe_query_id),
                has_attribute("speed", any_attribute().expires_at("2099-01-01T00:00:00Z".parse()?)),
                has_attribute("fuelLevel", any_attribute().value(70).without(Member::ExpiresAt))
            )
        )
    );
    broker.registrations().delete(&registration_id).send().await?;
    broker.registrations().delete(&registration_2_id).send().await?;
    Ok(())
}
