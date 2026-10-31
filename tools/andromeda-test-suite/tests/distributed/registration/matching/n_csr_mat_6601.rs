//! The attribute part matches when no names are asked none are registered or one is shared.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type, with no attrs, with attrs a registration does not name, with
/// attrs of which one is named, and over an entity-only registration, answer 200 with the registrations clause 5.12
/// admits
/// Clause: 5.10.2.4, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_mat_6601_the_attribute_part_matches_when_no_names_are_asked_none_are_registered_or_one_is_shared() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["brandName"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let registration_3 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker).registration(&registration_3_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    expect_that!(page.items(), contains(has_id(&registration_2_id)));
    expect_that!(page.items(), contains(has_id(&registration_3_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), contains(has_id(&registration_id)));
    expect_that!(page_2.items(), contains(has_id(&registration_3_id)));
    expect_that!(page_2.items(), not(contains(has_id(&registration_2_id))));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?, "fuelLevel".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), contains(has_id(&registration_id)));
    expect_that!(page_3.items(), contains(has_id(&registration_3_id)));
    expect_that!(page_3.items(), not(contains(has_id(&registration_2_id))));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["brandName".parse()?, "fuelLevel".parse()?])
        .send()
        .await?;
    expect_that!(page_4.items(), contains(has_id(&registration_2_id)));
    expect_that!(page_4.items(), contains(has_id(&registration_3_id)));
    expect_that!(page_4.items(), not(contains(has_id(&registration_id))));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .attrs(vec!["fuelLevel".parse()?])
        .send()
        .await?;
    expect_that!(page_5.items(), contains(has_id(&registration_3_id)));
    expect_that!(page_5.items(), not(contains(has_id(&registration_id))));
    expect_that!(page_5.items(), not(contains(has_id(&registration_2_id))));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(namespace.entity_type("Other")?)
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(page_6.items(), not(contains(has_id(&registration_id))));
    expect_that!(page_6.items(), not(contains(has_id(&registration_2_id))));
    expect_that!(page_6.items(), not(contains(has_id(&registration_3_id))));
    Ok(())
}
