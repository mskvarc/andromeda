//! Append partial success is an update result naming the registration.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    report_matcher::not_updated,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Append Entity Attributes of speed and colour under an exclusive registration that does not advertise
/// `appendAttrs` answers 207 with an `UpdateResult` whose `notUpdated` names speed and whose updated names the Fully Qualified Name of colour
/// Clause: 5.6.3.4, 5.2.18, 5.2.19, 6.3.6, 6.6.3.1
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6253_append_partial_success_is_an_update_result_naming_the_registration() -> Result<()> {
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
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fragment = EntityFragment::builder().property("speed", 99).property("colour", "red").build()?;
    let report = broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.partial()?;
    expect_that!(&report.not_updated, len(eq(1)));
    // Clause 6.3.6: a partial success body names each attribute by its Fully Qualified Name.
    expect_that!(
        report.updated.iter().map(ToString::to_string).collect::<Vec<_>>(),
        elements_are![eq("https://uri.etsi.org/ngsi-ld/default-context/colour")]
    );
    expect_that!(&report.not_updated, nth(0, not_updated().attribute("speed")));
    Ok(())
}
