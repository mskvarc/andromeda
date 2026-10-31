//! A query or retrieve whose single exclusive source answers 501 is answered 501.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, problem::answered, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities and Retrieve Entity whose only matching registration is an exclusive one whose source
/// answers 501 answer 501
/// Clause: 6.3.7, 6.4.3.2, 6.3.17
/// Recorded: clause 6.3.7 has a source answer 501 for a representation it cannot serve and clause 6.3.17 maps only
/// unsafe methods onto client statuses, so the status of a GET answered from one exclusive source is the broker
/// choice
#[gtest]
#[tokio::test]
async fn o_fed_err_7211_a_query_or_retrieve_whose_single_exclusive_source_answers_501_is_answered_501() -> Result<()> {
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
        .mode(RegistrationMode::Exclusive)
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
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_IMPLEMENTED));
    let answer_2 = broker.with_responses().with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await;
    expect_that!(&answer_2, answered(StatusCode::NOT_IMPLEMENTED));
    Ok(())
}
