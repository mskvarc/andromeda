//! Append attributes to an unrelated type is not refused.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment},
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Append Attributes on a local Entity whose type a redirect registration advertising only `retrieveEntity`
/// does not cover answers 204, the registration not being matched
/// Clause: 5.12, 5.6.3
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9003_append_attributes_to_an_unrelated_type_is_not_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let coverage_registered_type_type = namespace.entity_type("CoverageRegisteredType")?;
    let coverage_local_type_id = namespace.urn("CoverageLocalType", 1)?;
    let coverage_local_type_type = namespace.entity_type("CoverageLocalType")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(coverage_registered_type_type).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let coverage_local_type = Entity::builder()
        .id(coverage_local_type_id.clone())
        .entity_type(coverage_local_type_type)
        .property("speed", 10)
        .build()?;
    broker.entities().create(&coverage_local_type).send().await?.done()?;
    // The registration covers another type and advertises only retrieveEntity, so a write it were taken to cover would be refused.
    let fragment = EntityFragment::builder().property("fuel", 5).build()?;
    broker.entities().append_attributes(&coverage_local_type_id, &fragment).send().await?.done()?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
