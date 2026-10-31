//! A walk over registrations created in reverse returns each once ascending.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with limit 3 over 7 members created in descending identifier order,
/// followed by its next pointers, answers every member once in ascending identifier order, the last page with a
/// prev pointer and no next
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_csr_qry_0024_a_walk_over_registrations_created_in_reverse_returns_each_once_ascending() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_0006_id = namespace.registration_id("0006")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_0005_id = namespace.registration_id("0005")?;
    let registration_0004_id = namespace.registration_id("0004")?;
    let registration_0003_id = namespace.registration_id("0003")?;
    let registration_0002_id = namespace.registration_id("0002")?;
    let registration_0001_id = namespace.registration_id("0001")?;
    let registration_0000_id = namespace.registration_id("0000")?;
    let registration_0006 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0006_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0006).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_0006_cleanup = Cleanup::of(&broker).registration(&registration_0006_id);
    let registration_0005 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0005_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0005).send().await?;
    let _registration_0005_cleanup = Cleanup::of(&broker).registration(&registration_0005_id);
    let registration_0004 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0004_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0004).send().await?;
    let _registration_0004_cleanup = Cleanup::of(&broker).registration(&registration_0004_id);
    let registration_0003 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0003_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0003).send().await?;
    let _registration_0003_cleanup = Cleanup::of(&broker).registration(&registration_0003_id);
    let registration_0002 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0002_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0002).send().await?;
    let _registration_0002_cleanup = Cleanup::of(&broker).registration(&registration_0002_id);
    let registration_0001 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0001_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0001).send().await?;
    let _registration_0001_cleanup = Cleanup::of(&broker).registration(&registration_0001_id);
    let registration_0000 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_0000_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .property_names(["name"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_0000).send().await?;
    let _registration_0000_cleanup = Cleanup::of(&broker).registration(&registration_0000_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&building_type)
        .limit(3)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&registration_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&registration_0001_id)));
    expect_that!(page.items(), nth(2, has_id(&registration_0002_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_0003_id)));
    expect_that!(page_2.items(), nth(1, has_id(&registration_0004_id)));
    expect_that!(page_2.items(), nth(2, has_id(&registration_0005_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page_2.next_page().or_fail()?)
        .await?;
    expect_that!(page_3.next(), none());
    expect_that!(page_3.prev(), some(anything()));
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&registration_0006_id)));
    Ok(())
}
