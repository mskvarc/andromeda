//! Replace of an attribute the local entity does not hold under an inclusive registration is not 204.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Replace Attribute of an attribute the local entity does not hold at all is forwarded to the inclusive
/// registration that matches the entity, and the local `ResourceNotFound` is not dropped: the answer is not 204 (a
/// partial success or the error) and the local entity gains no attribute
/// Clause: 5.6.19.4, 6.7.3.3, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_5201_replace_of_an_attribute_the_local_entity_does_not_hold_under_an_inclusive_registration_is_not_204() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    source
        .answer_when(SourceRequest::any_matching("/.*").priority(1), source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::RedirectionOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let attribute = Attribute::from(Property::builder(99).build()?);
    let answer = broker
        .with_responses()
        .entities()
        .replace_attribute(&vehicle_2_id, &"altitude".parse::<AttributeName>()?, &attribute)
        .send()
        .await;
    expect_that!(answer.answered_status().map(|status| status.as_u16()), some(all!(not(eq(204)), ge(207))));
    let forwarded_request = SourceRequest::matching(Method::PUT, "/ngsi-ld/v1/entit.*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("altitude"));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    Ok(())
}
