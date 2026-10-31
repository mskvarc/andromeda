//! Exclusive create forwards the owned attribute and creates the rest locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    forwarded_body,
    forwarded_head,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, ContentType},
    model::{Entity, NgsiLdMediaType},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity carrying an owned speed and an unowned `brandName` under an exclusive registration of speed
/// answers 201, forwards one create carrying speed only as JSON, and creates the Entity locally with `brandName`
/// and no speed
/// Clause: 5.6.1.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6200_exclusive_create_forwards_the_owned_attribute_and_creates_the_rest_locally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::CreateEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entity = forwarded_body::entity(forwarded_one)?;
    expect_that!(&forwarded_entity, has_attribute("speed", any_attribute().value(42)));
    expect_that!(&forwarded_entity, lacks_attribute("brandName"));
    expect_that!(forwarded_entity.id, eq(&vehicle_id));
    expect_that!(
        forwarded_head::head(forwarded_one).typed::<ContentType>().valid(),
        some(any![
            eq(&ContentType::NgsiLd(NgsiLdMediaType::Json)),
            eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd))
        ])
    );
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(entity, lacks_attribute("speed"));
    Ok(())
}
