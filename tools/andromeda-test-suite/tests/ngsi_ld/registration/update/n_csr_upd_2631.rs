//! An empty fragment answers 204 and changes nothing.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationTypeMember},
};

/// Pins: Update Context Source Registration with the empty Fragment {} as application/json answers 204 and leaves
/// the registration as stored
/// Clause: 5.4, 5.9.3
/// Recorded: a Fragment is an IETF RFC 7396 Merge Patch, under which {} is valid and changes nothing
#[gtest]
#[tokio::test]
async fn n_csr_upd_2631_an_empty_fragment_answers_204_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .description("before")
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id(building_id)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragment = RegistrationFragment::builder().type_member(RegistrationTypeMember::Omitted).build()?;
    let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(registration_2.description(), some(eq("before")));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(building_type.clone())))
    );
    Ok(())
}
