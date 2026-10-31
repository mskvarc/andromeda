//! Inclusive append forwards the fragment and appends locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    forwarded_body,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Append Entity Attributes of speed and colour under an inclusive registration answers 204, forwards one
/// append carrying both, and the local Entity keeps `brandName` and gains speed and colour
/// Clause: 5.6.3.4, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6219_inclusive_append_forwards_the_fragment_and_appends_locally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::AppendAttrs, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::POST,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let fragment = EntityFragment::builder().property("speed", 99).property("colour", "red").build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs")).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entity = forwarded_body::fragment(forwarded_one)?;
    expect_that!(&forwarded_entity, has_attribute("speed", any_attribute().value(99)));
    expect_that!(&forwarded_entity, has_attribute("colour", any_attribute().value("red")));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(entity, has_attribute("speed", any_attribute().value(99)));
    expect_that!(entity, has_attribute("colour", any_attribute().value("red")));
    Ok(())
}
