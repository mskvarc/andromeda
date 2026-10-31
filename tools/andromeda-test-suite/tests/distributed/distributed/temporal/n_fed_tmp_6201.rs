//! Inclusive delete of a temporal attribute is forwarded and removes it locally.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Property, evolution::TemporalEntity},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Delete Attribute speed from a Temporal Evolution under an inclusive registration advertising
/// `deleteAttrsTemporal` answers 204, forwards one delete, and the local Temporal Evolution loses speed and keeps
/// `brandName`
/// Clause: 5.6.13.4, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_6201_inclusive_delete_of_a_temporal_attribute_is_forwarded_and_removes_it_locally() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("Mercedes").observed_at("2021-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(42).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_evolution)
        .locality(Locality::Local)
        .send()
        .await?
        .created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::DeleteAttrsTemporal, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::DELETE,
            &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}/attrs/speed"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    broker
        .temporal()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await?;
    let forwarded = source
        .received_on(Method::DELETE, &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}/attrs/speed"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, lacks_evolution("speed"));
    expect_that!(evolution, has_evolution("brandName", anything()));
    Ok(())
}
