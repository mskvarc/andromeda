//! A query or retrieve source answering 501 adds warning 299 and no error status.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, WarningCode},
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities and Retrieve Entity of a locally held Entity whose inclusive source answers 501 answer 200
/// with the local Entity and NGSILD-Warning code 299
/// Clause: 6.3.17, 6.3.7, 5.7.1.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_err_7208_a_query_or_retrieve_source_answering_501_adds_warning_299_and_no_error_status() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "x")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::NOT_IMPLEMENTED))
        .await?;
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::status(StatusCode::NOT_IMPLEMENTED),
        )
        .await?;
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(
        page.warnings().valid().and_then(|warnings| warnings.iter().next()).map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousPersistentWarning))
    );
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(
        retrieved
            .warnings()
            .valid()
            .and_then(|warnings| warnings.iter().next())
            .map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousPersistentWarning))
    );
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("x")));
    Ok(())
}
