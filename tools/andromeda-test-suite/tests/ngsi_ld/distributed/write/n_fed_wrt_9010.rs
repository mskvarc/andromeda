//! A registration covering a second entity type is matched.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Entity, EntityFragment},
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Attributes on a local Entity of two types, under an inclusive `updateEntity` registration covering
/// the second, answers 204 and forwards the update to the source as a PATCH of the attributes
/// Clause: 5.12, 5.6.2.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9010_a_registration_covering_a_second_entity_type_is_matched() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let multi_type_id = namespace.urn("MultiType", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let multi_type_second_type = namespace.entity_type("MultiTypeSecond")?;
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/entities/{multi_type_id}/attrs"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(multi_type_second_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
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
        .entity_type(vec![namespace.entity_type("MultiTypeFirst")?, multi_type_second_type.clone()])
        .property("speed", 10)
        .build()?;
    broker.entities().create(&multi_type).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 20).build()?;
    broker.entities().update_attributes(&multi_type_id, &fragment).send().await?.done()?;
    let forwarded = source
        .received_on(Method::PATCH, &format!("/a/ngsi-ld/v1/entities/{multi_type_id}/attrs"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
