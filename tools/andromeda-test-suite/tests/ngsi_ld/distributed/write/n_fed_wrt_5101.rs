//! Partial update of an attribute the local entity lacks under an inclusive registration is not 204.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    source_answer,
    source_request::SourceRequest,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Partial Attribute Update of an attribute whose only local instance carries a `datasetId` (no default instance)
/// is forwarded to the inclusive registration that matches the entity, and the local `ResourceNotFound` is not dropped:
/// the answer is not 204, the local entity is unchanged, and the same request on an entity no registration matches
/// answers 404
/// Clause: 5.6.4.4, 6.7.3.1, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_5101_partial_update_of_an_attribute_the_local_entity_lacks_under_an_inclusive_registration_is_not_204() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_id = namespace.registration_id(1)?;
    let entities = [
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(56).dataset_id(namespace.dataset_id("Property", "a")?))
            .build()?,
        Entity::builder()
            .id(vehicle_3_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(56).dataset_id(namespace.dataset_id("Property", "a")?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
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
    // the control: no registration matches this entity
    // The body names the attribute inside the fragment, which the model does not write, so it is sent as written.
    let request_control = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_3_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from_static(br#"{"speed": {"type": "Property", "value": 99}}"#));
    let answer_control = broker.send_unchecked(request_control).await;
    expect_that!(&answer_control, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
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
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().value(56).dataset_id(namespace.dataset_id("Property", "a")?))
    );
    Ok(())
}
