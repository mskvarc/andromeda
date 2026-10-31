//! Partial update of an attribute the local entity does not hold under an inclusive registration is not 204.

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
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Partial Attribute Update of an attribute the local entity does not hold at all is forwarded to the inclusive
/// registration that matches the entity, and the local `ResourceNotFound` is not dropped: the answer is not 204 and the
/// local entity gains no attribute
/// Clause: 5.6.4.4, 6.7.3.1, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_5102_partial_update_of_an_attribute_the_local_entity_does_not_hold_under_an_inclusive_registration_is_not_204() -> Result<()> {
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
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_2_id.clone())])
            .build()?])
        .operations([OperationGroup::RedirectionOps])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // The body names the attribute inside the fragment, which the model does not write, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_2_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from_static(br#"{"speed": {"type": "Property", "value": 99}}"#));
    let answer = broker.send_unchecked(request).await;
    expect_that!(answer.answered_status(), some(ge(StatusCode::MULTI_STATUS)));
    let forwarded_request = SourceRequest::matching(Method::PATCH, "/ngsi-ld/v1/entit.*");
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_2_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(entity, lacks_attribute("speed"));
    Ok(())
}
