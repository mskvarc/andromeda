//! A linked entity the source fails to return is left out with a warning.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, WarningCode},
    model::{Entity, Relationship, VocabTerm},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: A flat join whose Linked Entity source answers 500 answers 200 with the root alone and an NGSILD-Warning with
/// code 299
/// Clause: 4.5.23, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_ret_0008_a_linked_entity_the_source_fails_to_return_is_left_out_with_a_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let building_a_id = namespace.urn("Building", "A")?;
    let building_type = namespace.entity_type("Building")?;
    let building_r_id = namespace.urn("Building", "R")?;
    let registration_id = namespace.registration_id(1)?;
    let building_a = Entity::builder()
        .id(building_a_id.clone())
        .entity_type(building_type.clone())
        .relationship(
            "linkedTo",
            Relationship::builder(building_r_id.clone()).object_type(VocabTerm::from(&building_type)),
        )
        .build()?;
    broker.entities().create(&building_a).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id(building_r_id.clone())])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/entities/.*-R"),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve_flattened(&building_a_id)
        .entity_type(&building_type)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), len(eq(1)));
    expect_that!(entities.as_slice(), contains(has_id(&building_a_id)));
    expect_that!(
        retrieved
            .warnings()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousPersistentWarning))
    );
    Ok(())
}
