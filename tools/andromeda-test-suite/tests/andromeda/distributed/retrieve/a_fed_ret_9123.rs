//! A source answering a retrieve with an array adds nothing to it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdWarning, WarningCode},
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity of a local entity whose inclusive source answers with an array, even one holding the
/// requested entity, answers the local attributes alone and a 111 NGSILD-Warning naming the source
/// Clause: 5.7.1.4, 6.3.17
/// Recorded: clause 5.7.1.4 merges what the sources return for the requested entity, which a retrieval answers as one
/// Entity, and 6.3.17 leaves the warning text to the broker; Andromeda names the registration and says the payload is
/// no entity document
#[gtest]
#[tokio::test]
async fn a_fed_ret_9123_a_source_answering_a_retrieve_with_an_array_adds_nothing_to_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let car_type = namespace.entity_type("Car")?;
    let car_a = namespace.urn("Car", "a")?;
    let car_b = namespace.urn("Car", "b")?;
    let registration_id = namespace.registration_id("1")?;
    let car = Entity::builder()
        .id(car_a.clone())
        .entity_type(car_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&car).send().await?.done()?;
    let path = format!("/current/ngsi-ld/v1/entities/{car_a}");
    // The source's answer is the subject: an array where one entity belongs, written as the source sends it.
    let answer = format!(
        r#"[
{{"id": "{car_a}", "type": "{car_type}", "owner": {{"type": "Relationship", "object": "urn:ngsi-ld:Person:{namespace}-1"}}}},
{{"id": "{car_b}", "type": "{car_type}", "owner": {{"type": "Relationship", "object": "urn:ngsi-ld:Person:{namespace}-2"}}}}
]"#
    );
    source
        .answer_when(
            SourceRequest::new(Method::GET, &path),
            source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &answer),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("current")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(car_type.clone())]).build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&car_a)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_id(&car_a));
    expect_that!(entity, has_attribute("speed", any_attribute().value(50)));
    expect_that!(entity, lacks_attribute("owner"));
    let warnings = retrieved.warnings().valid().or_fail()?;
    expect_that!(
        warnings.iter().map(NgsiLdWarning::code).collect::<Vec<_>>(),
        elements_are![eq(&WarningCode::RevalidationFailed)]
    );
    expect_that!(
        warnings.iter().map(NgsiLdWarning::text).collect::<Vec<_>>(),
        elements_are![all!(
            contains_substring(registration_id.to_string()),
            contains_substring("returned a payload that is no entity document")
        )]
    );
    broker.registrations().delete(&registration_id).send().await?;
    broker.entities().delete(&car_a).send().await?;
    Ok(())
}
