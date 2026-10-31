//! A create is validated before it is forwarded.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity whose only Property, claimed by an exclusive registration, has the value urn:ngsi-ld:null
/// answers 400 `BadRequestData`, forwards nothing and stores nothing, while the same create with a value is
/// forwarded to the source
/// Clause: 5.6.1.4, 5.5.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9012_a_create_is_validated_before_it_is_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_null_sentinel_exclusive_id = namespace.registration_id("null-sentinel-exclusive")?;
    let building_proxied_null_id = namespace.urn("Building", "proxied-null")?;
    let null_sentinel_proxied_probe_type = namespace.entity_type("NullSentinelProxiedProbe")?;
    let building_proxied_ok_id = namespace.urn("Building", "proxied-ok")?;
    source
        .answer(Method::POST, "/exclusive/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let registration_null_sentinel_exclusive = CSourceRegistration::builder(source.endpoint("exclusive")?)
        .id(registration_null_sentinel_exclusive_id.clone())
        .information([RegistrationInfo::builder()
            .entities([
                EntityInfo::new(null_sentinel_proxied_probe_type.clone()).with_id(building_proxied_null_id.clone()),
                EntityInfo::new(null_sentinel_proxied_probe_type.clone()).with_id(building_proxied_ok_id.clone()),
            ])
            .property_names(["temperature"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    broker.registrations().register(&registration_null_sentinel_exclusive).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_null_sentinel_exclusive_cleanup = Cleanup::of(&broker).registration(&registration_null_sentinel_exclusive_id);
    let building_proxied_null = Entity::builder()
        .id(building_proxied_null_id.clone())
        .entity_type(null_sentinel_proxied_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in `value` at temperature), so `temperature` is written as text.
    let temperature: RawJson = r#"{"type": "Property", "value": "urn:ngsi-ld:null"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building_proxied_null, Lossless::Normalized).set_member("temperature", &temperature);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_proxied_null).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let forwarded = source.received_on(Method::POST, "/exclusive/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(0)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_proxied_null_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let building_proxied_ok = Entity::builder()
        .id(building_proxied_ok_id.clone())
        .entity_type(null_sentinel_proxied_probe_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_proxied_ok).send().await?.done()?;
    let forwarded_2 = source.received_on(Method::POST, "/exclusive/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(1)));
    broker.registrations().delete(&registration_null_sentinel_exclusive_id).send().await?;
    Ok(())
}
