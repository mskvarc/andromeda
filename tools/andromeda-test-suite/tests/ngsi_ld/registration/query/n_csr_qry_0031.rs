//! The last page reached by next offers only prev.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with limit 2 over four members, followed by its next pointer, answers
/// the last two members with a prev pointer and no first, last or next pointer
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_csr_qry_0031_the_last_page_reached_by_next_offers_only_prev() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_0000_id = namespace.registration_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_0001_id = namespace.registration_id("0001")?;
    let registration_0002_id = namespace.registration_id("0002")?;
    let registration_0003_id = namespace.registration_id("0003")?;
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&registration_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&registration_0001_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.prev(), some(anything()));
    expect_that!(page_2.first(), none());
    expect_that!(page_2.last(), none());
    expect_that!(page_2.next(), none());
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&registration_0003_id)));
    Ok(())
}
