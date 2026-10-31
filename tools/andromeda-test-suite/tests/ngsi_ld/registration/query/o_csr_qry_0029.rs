//! The last pointer is at the offset of the final page.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, page_link::carries, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type with limit 2 and offset 0 over five registrations answers a
/// last pointer at offset 4
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 6.3.10 names only the next and prev link-values; Andromeda also offers first and last, and never a
/// pointer of a page to itself
/// Recorded: 6.3.10 says the pointers of the transparent option "should include" limit and offset; Andromeda keeps
/// offset pointers when the request names an offset or an order
#[gtest]
#[tokio::test]
async fn o_csr_qry_0029_the_last_pointer_is_at_the_offset_of_the_final_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_0000_id = namespace.registration_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_0001_id = namespace.registration_id("0001")?;
    let registration_0002_id = namespace.registration_id("0002")?;
    let registration_0003_id = namespace.registration_id("0003")?;
    let registration_0004_id = namespace.registration_id("0004")?;
    let registration_0000 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0000_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0000).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_0000_cleanup = Cleanup::of(&broker).registration(&registration_0000_id);
    let registration_0001 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0001_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0001).send().await?;
    let _registration_0001_cleanup = Cleanup::of(&broker).registration(&registration_0001_id);
    let registration_0002 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0002_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0002).send().await?;
    let _registration_0002_cleanup = Cleanup::of(&broker).registration(&registration_0002_id);
    let registration_0003 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0003_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0003).send().await?;
    let _registration_0003_cleanup = Cleanup::of(&broker).registration(&registration_0003_id);
    let registration_0004 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0004_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0004).send().await?;
    let _registration_0004_cleanup = Cleanup::of(&broker).registration(&registration_0004_id);
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registrations()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .offset(0)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(page.last(), some(carries("offset", "4")));
    Ok(())
}
