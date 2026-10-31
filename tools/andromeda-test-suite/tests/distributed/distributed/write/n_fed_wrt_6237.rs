//! Delete of an attribute held only by a registration is forwarded.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::AttributeName,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Delete Attribute speed with `deleteAll` true for an id the broker does not hold, matched by an exclusive
/// registration of speed, answers 204 and forwards one delete to the attribute path
/// Clause: 5.6.5.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6237_delete_of_an_attribute_held_only_by_a_registration_is_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
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
    Ok(())
}
