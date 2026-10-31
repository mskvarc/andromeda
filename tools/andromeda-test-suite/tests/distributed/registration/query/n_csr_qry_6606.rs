//! A context source filter matches a source property of the registration.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson},
    query::q,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type and a csf over tier answers 200 with the registration whose
/// tier property satisfies it and without the one whose tier differs
/// Clause: 5.10.2.4, 4.9
#[gtest]
#[tokio::test]
async fn n_csr_qry_6606_a_context_source_filter_matches_a_source_property_of_the_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration a registration property), so `tier` is written as text.
    let tier: RawJson = r#"{"type": "Property", "value": "gold"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("tier", &tier);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration a registration property), so `tier` is written as text.
    let tier_2: RawJson = r#"{"type": "Property", "value": "silver"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("tier", &tier_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .csf(q::attr("tier").eq("gold"))
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    expect_that!(page.items(), not(contains(has_id(&registration_2_id))));
    Ok(())
}
