//! A concise member is an attribute that takes the expiry.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    source_answer,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Entity merging a version with `expiresAt` 2099-01-01 holding speed and a bare `vendorNote` and a
/// version without `expiresAt` answers `vendorNote` as a Property holding its text, and both with `expiresAt`
/// 2099-01-01
/// Clause: 4.5.5.2, 4.5.2.3, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_9013_a_concise_member_is_an_attribute_that_takes_the_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let transient_probe_concise_id = namespace.urn("TransientProbe", "concise")?;
    let transient_probe_type = namespace.entity_type("TransientProbe")?;
    // A concise member beside the Property members is not an entity the model reads, so the answer is written as text.
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/entities/{transient_probe_concise_id}"),
            source_answer::text(
                StatusCode::OK,
                HeaderValue::from_static("application/json"),
                &format!(
                    r#"{{"id": "urn:ngsi-ld:TransientProbe:{namespace}-concise", "type": "TransientProbe_{namespace}", "expiresAt": "2099-01-01T00:00:00Z", "speed": {{"type": "Property", "value": 60}}, "vendorNote": "carried through verbatim"}}"#
                ),
            ),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type.clone()).with_id(transient_probe_concise_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let source_entity = Entity::output_builder()
        .id(transient_probe_concise_id.clone())
        .entity_type(transient_probe_type.clone())
        .property("fuelLevel", 70)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/b/ngsi-ld/v1/entities/{transient_probe_concise_id}"),
            source_answer::entity(&source_entity),
        )
        .await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(transient_probe_type).with_id(transient_probe_concise_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&transient_probe_concise_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().expires_at("2099-01-01T00:00:00Z".parse()?)));
    expect_that!(
        entity,
        has_attribute(
            "vendorNote",
            any_attribute().value("carried through verbatim").expires_at("2099-01-01T00:00:00Z".parse()?)
        )
    );
    broker.registrations().delete(&namespace.registration_id(1)?).send().await?;
    broker.registrations().delete(&namespace.registration_id(2)?).send().await?;
    Ok(())
}
