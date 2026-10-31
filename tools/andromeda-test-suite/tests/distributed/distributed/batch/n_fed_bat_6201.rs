//! Create keeps a redirect source to the attributes it registers.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    forwarded_body,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Creation of a Vehicle with `brandName` and speed under redirect registrations of speed and of
/// `brandName` forwards the `brandName` source one batch holding `brandName` alone
/// Clause: 5.6.7.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_bat_6201_create_keeps_a_redirect_source_to_the_attributes_it_registers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["brandName"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/create",
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    source
        .answer(
            Method::POST,
            "/s2/ngsi-ld/v1/entityOperations/create",
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    let entities = [Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .property("speed", 42)
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/s2/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entities = forwarded_body::entities(forwarded_one)?;
    let forwarded_item = forwarded_entities.first().or_fail()?;
    expect_that!(forwarded_item, lacks_attribute("speed"));
    expect_that!(forwarded_item, has_attribute("brandName", any_attribute().value("Mercedes")));
    let forwarded_2 = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_one_2 = forwarded_2.first().or_fail()?;
    let forwarded_entities_2 = forwarded_body::entities(forwarded_one_2)?;
    let forwarded_item_2 = forwarded_entities_2.first().or_fail()?;
    expect_that!(forwarded_item_2, lacks_attribute("brandName"));
    expect_that!(forwarded_item_2, has_attribute("speed", any_attribute().value(42)));
    Ok(())
}
