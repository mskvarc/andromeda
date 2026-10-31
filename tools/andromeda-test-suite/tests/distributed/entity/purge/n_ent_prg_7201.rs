//! A purge of every type is local and contacts no source.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::builders::SelectorTypeBuilder,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Purge Entities with type * and an id over a matching registration advertising `purgeEntity` deletes the
/// local Entity, answers 204 and the source receives no request
/// Clause: 5.6.21.4, 6.3.18, Table 6.4.3.3-1
#[gtest]
#[tokio::test]
async fn n_ent_prg_7201_a_purge_of_every_type_is_local_and_contacts_no_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::PurgeEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer_when(SourceRequest::any_matching("/.*"), source_answer::status(StatusCode::OK))
        .await?;
    broker
        .entities()
        .purge()
        .entity_type(SelectorTypeBuilder::All)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?
        .done()?;
    let answer = broker.with_responses().with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let forwarded_request = SourceRequest::any_matching("/.*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(0)));
    Ok(())
}
