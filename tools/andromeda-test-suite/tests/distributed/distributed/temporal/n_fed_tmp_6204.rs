//! Inclusive modification of an instance is forwarded and applied locally.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Property, evolution::TemporalEntity},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Partial Update of the local Attribute instance of speed under an inclusive registration advertising
/// `updateAttrInstanceTemporal` answers 204, forwards one patch to the instance path, and the local instance takes
/// the new value
/// Clause: 5.6.14.4, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_6204_inclusive_modification_of_an_instance_is_forwarded_and_applied_locally() -> Result<()> {
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
        .operations([DistributedOperation::UpdateAttrInstanceTemporal, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let retrieved = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", nth(0, any_attribute().with_member(Member::InstanceId))));
    let instance = instance_id_at(evolution, &"speed".parse()?, 0)?;
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}/attrs/speed/{instance}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let instance_2 = Attribute::from(Property::builder(100).observed_at("2021-03-01T00:00:00Z".parse()?).build()?);
    broker
        .temporal()
        .modify_instance(&vehicle_id, &"speed".parse::<AttributeName>()?, &instance, &instance_2)
        .send()
        .await?;
    let forwarded = source
        .received_on(Method::PATCH, &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}/attrs/speed/{instance}"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("speed", nth(0, any_attribute().value(100))));
    Ok(())
}
