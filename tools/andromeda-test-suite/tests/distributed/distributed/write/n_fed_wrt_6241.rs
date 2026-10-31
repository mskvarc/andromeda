//! Inclusive delete of an attribute forwards the delete and removes it locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, Entity},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Delete Attribute speed with `deleteAll` true under an inclusive registration answers 204, forwards one
/// delete, and the local Entity loses speed and keeps `brandName`
/// Clause: 5.6.5.4, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6241_inclusive_delete_of_an_attribute_forwards_the_delete_and_removes_it_locally() -> Result<()> {
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
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::DeleteAttrs, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::DELETE,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .instances(InstanceSelection::All)
        .send()
        .await?
        .done()?;
    let forwarded = source
        .received_on(Method::DELETE, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    Ok(())
}
