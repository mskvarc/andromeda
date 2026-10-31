//! A csf on mode reads the registration mode and its default.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::q,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Context Source Registrations by type with csf mode=="exclusive" answers only the exclusive
/// registration, and with csf mode=="inclusive" only the registration written without a mode, which defaults to
/// inclusive
/// Clause: 4.9, 5.10.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_qry_9012_a_csf_on_mode_reads_the_registration_mode_and_its_default() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_exclusive_id = namespace.registration_id("exclusive")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_inclusive_id = namespace.registration_id("inclusive")?;
    let registration_exclusive = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_exclusive_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .build()?;
    broker.registrations().register(&registration_exclusive).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_exclusive_cleanup = Cleanup::of(&broker).registration(&registration_exclusive_id);
    let registration_inclusive = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_inclusive_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_inclusive).send().await?;
    let _registration_inclusive_cleanup = Cleanup::of(&broker).registration(&registration_inclusive_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .csf(q::attr("mode").eq("exclusive"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_exclusive_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .csf(q::attr("mode").eq("inclusive"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_inclusive_id)));
    Ok(())
}
