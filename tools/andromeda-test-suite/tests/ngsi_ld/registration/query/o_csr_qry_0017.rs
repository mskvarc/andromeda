//! The default first page offers opaque cursor pointers.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with limit 2 over three members answers a first page whose Link offers
/// next and last pointers carrying a cursor and no offset, and no first or prev
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor and offers first and last pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_csr_qry_0017_the_default_first_page_offers_opaque_cursor_pointers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_0000_id = namespace.registration_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_0001_id = namespace.registration_id("0001")?;
    let registration_0002_id = namespace.registration_id("0002")?;
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.next(), some(anything()));
    expect_that!(page.last(), some(anything()));
    expect_that!(page.first(), none());
    expect_that!(page.prev(), none());
    expect_that!(links(&page), contains(carries_parameter("cursor")));
    expect_that!(links(&page), each(not(carries_parameter("offset"))));
    Ok(())
}
