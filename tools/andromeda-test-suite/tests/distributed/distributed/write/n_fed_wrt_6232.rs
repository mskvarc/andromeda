//! Exclusive replace of an attribute forwards the fragment and leaves the local value.

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
    model::{Attribute, AttributeName, Entity, Property},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Replace Attribute of speed under an exclusive registration of speed answers 204, forwards one put to the
/// attribute path, and the local speed keeps its value
/// Clause: 5.6.19.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6232_exclusive_replace_of_an_attribute_forwards_the_fragment_and_leaves_the_local_value() -> Result<()> {
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
        .operations([DistributedOperation::ReplaceAttrs, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    source
        .answer(
            Method::PUT,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let attribute = Attribute::from(Property::builder(99).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let forwarded = source
        .received_on(Method::PUT, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entity = forwarded_body::attribute(forwarded_one, "speed")?;
    expect_that!(forwarded_entity, has_attribute("speed", any_attribute().value(99)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(42)));
    Ok(())
}
