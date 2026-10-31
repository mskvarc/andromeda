//! Exclusive modification of an instance is forwarded with the fragment.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    forwarded_body,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Attribute, AttributeName, InstanceId, Property, evolution::TemporalEntity},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Partial Update of an Attribute instance of speed under an exclusive registration of speed advertising
/// `updateAttrInstanceTemporal` answers 204 and forwards one patch to the instance path carrying a fragment that
/// holds speed as an array of one instance
/// Clause: 5.6.14.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_tmp_6202_exclusive_modification_of_an_instance_is_forwarded_with_the_fragment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let attr_instance_id = namespace.urn("attrInstance", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::UpdateAttrInstanceTemporal, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
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
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}/attrs/speed/{attr_instance_id}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let instance_id = format!("urn:ngsi-ld:attrInstance:{namespace}-1").parse::<InstanceId>()?;
    let instance = Attribute::from(Property::builder(100).observed_at("2021-03-01T00:00:00Z".parse()?).build()?);
    broker
        .temporal()
        .modify_instance(&vehicle_id, &"speed".parse::<AttributeName>()?, &instance_id, &instance)
        .send()
        .await?;
    let forwarded = source
        .received_on(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}/attrs/speed/{attr_instance_id}"),
        )
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_evolution = forwarded_body::evolution_fragment(forwarded_one)?;
    expect_that!(
        &forwarded_evolution,
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(100))))
    );
    Ok(())
}
