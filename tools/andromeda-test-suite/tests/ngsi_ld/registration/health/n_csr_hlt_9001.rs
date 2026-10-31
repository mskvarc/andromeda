//! A registration that forwarded nothing reports none of the five members.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: A registration that has triggered no distributed operation carries none of status, `timesSent`,
/// `timesFailed`, `lastSuccess` and `lastFailure`, on Retrieve and in Query Context Source Registrations
/// Clause: Table 5.2.9-2, 5.10.1, 5.10.2
#[gtest]
#[tokio::test]
async fn n_csr_hlt_9001_a_registration_that_forwarded_nothing_reports_none_of_the_five_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_fresh_id = namespace.registration_id("fresh")?;
    let vehicle_fresh_id = namespace.urn("Vehicle", "fresh")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_fresh = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_fresh_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_fresh_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_fresh).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_fresh_cleanup = Cleanup::of(&broker).registration(&registration_fresh_id);
    let registration = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_fresh_id)
        .send()
        .await?;
    expect_that!(registration.output().status, none());
    expect_that!(registration.output().times_sent, none());
    expect_that!(registration.output().times_failed, none());
    expect_that!(registration.output().last_success, none());
    expect_that!(registration.output().last_failure, none());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_fresh_id.clone()])
        .send()
        .await?;
    let listed = page.items().first().or_fail()?;
    expect_that!(listed.output().status, none());
    expect_that!(listed.output().times_sent, none());
    expect_that!(listed.output().times_failed, none());
    expect_that!(listed.output().last_success, none());
    expect_that!(listed.output().last_failure, none());
    expect_that!(page.items(), len(eq(1)));
    broker.registrations().delete(&registration_fresh_id).send().await?;
    Ok(())
}
