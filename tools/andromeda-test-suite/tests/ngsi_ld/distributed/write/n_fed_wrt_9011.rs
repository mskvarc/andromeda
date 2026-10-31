//! A registration covering none of the entity types is not matched.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_request::SourceRequest};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment},
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Attributes on a local Entity of two types, under an inclusive `updateEntity` registration covering
/// a third, answers 204 and forwards nothing
/// Clause: 5.12, 5.6.2.4
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9011_a_registration_covering_none_of_the_entity_types_is_not_matched() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let multi_type_unrelated_type = namespace.entity_type("MultiTypeUnrelated")?;
    let multi_type_id = namespace.urn("MultiType", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(multi_type_unrelated_type).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::UpdateEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let multi_type = Entity::builder()
        .id(multi_type_id.clone())
        .entity_type(vec![namespace.entity_type("MultiTypeFirst")?, namespace.entity_type("MultiTypeSecond")?])
        .property("speed", 10)
        .build()?;
    broker.entities().create(&multi_type).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 20).build()?;
    broker.entities().update_attributes(&multi_type_id, &fragment).send().await?.done()?;
    let forwarded_request = SourceRequest::any_matching("/a/.*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(0)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
