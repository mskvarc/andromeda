//! A replace whose inclusive source answers 500 is a 207 with the failing registration and the replacement held
//! locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    report_matcher::batch_error,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{BatchOutcome, headers::Accept, operation::EntityIdMember},
    model::Entity,
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Replace Entity matching an inclusive registration whose source answers 500 answers 207 with a
/// `BatchOperationResult` holding the entity as success and the registration as the one error, and the replacement
/// is held locally
/// Clause: 5.6.18.4, 6.3.17, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_fed_wrt_7201_a_replace_whose_inclusive_source_answers_500_is_a_207_with_the_failing_registration_and_the_replacement_held_locally() -> Result<()> {
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
        .property("brandName", "x")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::ReplaceEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::PUT,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 9)
        .build()?;
    let received = broker
        .with_responses()
        .entities()
        .replace(&replacement)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await?;
    expect_that!(received.status, eq(StatusCode::MULTI_STATUS));
    let report = match received.outcome {
        BatchOutcome::Partial(report) => Some(report),
        BatchOutcome::Done => None,
    }
    .or_fail()?;
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&vehicle_id)));
    expect_that!(&report.errors, nth(0, batch_error().registration(&registration_id).entity(&vehicle_id)));
    let forwarded = source.received_on(Method::PUT, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(9)));
    Ok(())
}
