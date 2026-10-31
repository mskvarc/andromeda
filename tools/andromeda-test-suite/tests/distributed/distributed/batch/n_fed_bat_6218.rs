//! Update forwards a source only the entities carrying an attribute it registers.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    forwarded_body,
    namespace::Namespace,
    partial_outcome::PartialSuccess,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::EntityFragment,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Update of a Vehicle with speed and of one with `brandName` alone, under redirect
/// registrations of speed and of gear, forwards the speed source one batch holding only the speed Vehicle and the
/// gear source nothing
/// Clause: 5.6.9.4, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_bat_6218_update_forwards_a_source_only_the_entities_carrying_an_attribute_it_registers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::UpdateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["gear"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::UpdateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/update",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    source
        .answer(
            Method::POST,
            "/s2/ngsi-ld/v1/entityOperations/update",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let fragments = [
        EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
            .property("speed", 42)
            .build()?,
        EntityFragment::for_entity(vehicle_2_id, vehicle_type.clone())
            .property("brandName", "Mercedes")
            .build()?,
    ];
    broker.entities().batch_update(&fragments).send().await?.partial()?;
    let forwarded = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/update").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entities = forwarded_body::entities(forwarded_one)?;
    expect_that!(forwarded_entities, len(eq(1)));
    let forwarded_item = forwarded_entities.first().or_fail()?;
    expect_that!(forwarded_item.id, eq(&vehicle_id));
    expect_that!(forwarded_item, has_attribute("speed", any_attribute().value(42)));
    let forwarded_2 = source.received_on(Method::POST, "/s2/ngsi-ld/v1/entityOperations/update").await?;
    expect_that!(forwarded_2, len(eq(0)));
    Ok(())
}
