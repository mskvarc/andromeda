//! A registration matches only on the item declaring both the type and the attribute.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Context Source Registrations over a registration with one item of type A declaring p1 and another of
/// type B declaring p2 answers nothing for type A with attrs p2, and the registration for type A with attrs p1
/// Clause: 5.10.2.4, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_qry_0032_a_registration_matches_only_on_the_item_declaring_both_the_type_and_the_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let a_type = namespace.entity_type("A")?;
    let b_type = namespace.entity_type("B")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([a_type.as_str()])
                .property_names([namespace.attribute("p1")?.to_string()])
                .build()?,
            RegistrationInfo::builder()
                .entities([b_type.as_str()])
                .property_names([namespace.attribute("p2")?.to_string()])
                .build()?,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // Clause 5.12: the type and the attribute names must match the same
    // RegistrationInfo element.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&a_type)
        .attrs(vec![format!("p2_{namespace}").parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&a_type)
        .attrs(vec![format!("p1_{namespace}").parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_id)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
