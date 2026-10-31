//! An updated case insensitive id pattern replaces the old one in discovery.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityId, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationTypeMember},
};

/// Pins: after Update Context Source Registration replaces an `idPattern` folding hive with one folding wasp,
/// discovery finds the registration for WASP:1 and no longer for HIVE:1
/// Clause: 4.9, 5.9.3.4, 5.10.2.4, 5.12
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_csr_upd_0001_an_updated_case_insensitive_id_pattern_replaces_the_old_one_in_discovery() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let hive_type = namespace.entity_type("Hive")?;
    let ngsi_ld_hive_id = format!("urn:ngsi-ld:Hive:{namespace}:HIVE:1").parse::<EntityId>()?;
    let ngsi_ld_hive_id_2 = format!("urn:ngsi-ld:Hive:{namespace}:WASP:1").parse::<EntityId>()?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(hive_type.clone()).with_id_pattern(format!("^urn:ngsi-ld:Hive:{namespace}:(?i:hive):").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&hive_type)
        .ids(vec![ngsi_ld_hive_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let fragment = RegistrationFragment::builder()
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(hive_type.clone()).with_id_pattern(format!("^urn:ngsi-ld:Hive:{namespace}:(?i:wasp):").parse::<IdPattern>()?)])
            .build()?])
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&hive_type)
        .ids(vec![ngsi_ld_hive_id_2])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&hive_type)
        .ids(vec![ngsi_ld_hive_id.clone()])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
