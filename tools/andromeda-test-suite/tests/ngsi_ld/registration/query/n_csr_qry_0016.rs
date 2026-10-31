//! A page reached backward holding the first members offers no prev.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with limit 2 over four members, followed by next and then by the second
/// page's prev pointer, answers the first two members with no prev pointer, there being no previous members
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_csr_qry_0016_a_page_reached_backward_holding_the_first_members_offers_no_prev() -> Result<()> {
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
    expect_that!(page.prev(), none());
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&registration_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&registration_0001_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.prev(), some(anything()));
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&registration_0003_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page_2.prev_page().or_fail()?)
        .await?;
    expect_that!(page_3.prev(), none());
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&registration_0000_id)));
    expect_that!(page_3.items(), nth(1, has_id(&registration_0001_id)));
    Ok(())
}
