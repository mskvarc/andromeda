//! A registration whose alias the via header lists does not match.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, Via, ViaEntry},
        operation::RegisterContextSource,
    },
    model::{NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type with a Via header naming the `contextSourceAlias` of a
/// registration answers 200 without that registration and with the one carrying another alias, and without the
/// header with both
/// Clause: 5.12, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_csr_mat_6603_a_registration_whose_alias_the_via_header_lists_does_not_match() -> Result<()> {
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
    // The model refuses it (a registration contextSourceAlias), so `contextSourceAlias` is written as text.
    let context_source_alias: RawJson = format!(r#""loop-{namespace}""#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceAlias", &context_source_alias);
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
    // The model refuses it (a registration contextSourceAlias), so `contextSourceAlias` is written as text.
    let context_source_alias_2: RawJson = format!(r#""free-{namespace}""#).parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("contextSourceAlias", &context_source_alias_2);
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
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    expect_that!(page.items(), contains(has_id(&registration_2_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(Via::try_from_entries([format!("1.1 loop-{namespace}").parse::<ViaEntry>()?])?)?
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), contains(has_id(&registration_2_id)));
    expect_that!(page_2.items(), not(contains(has_id(&registration_id))));
    Ok(())
}
