//! A csf existence term reads a sub-property of a context source property.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    core_context::core_context_list,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{EntityTypeName, NgsiLdMediaType, RawJson},
    query::{IdPattern, q},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: over a registration whose Context Source Property `a` holds the sub-property `b` and a registration whose
/// `a` holds none, Query Context Source Registrations by type with csf a.b answers only the first; an existence term
/// selects what defines the target element its whole path names, and `a.b` names the sub-property
/// Clause: 4.9, 5.10.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_qry_9091_a_csf_existence_term_reads_a_sub_property_of_a_context_source_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_type: EntityTypeName = format!("https://test.andromeda/{namespace}/Device").parse()?;
    let nested_id = namespace.registration_id("nested")?;
    let flat_id = namespace.registration_id("flat")?;
    let nested = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(nested_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone())
                .with_id_pattern(format!("urn:ngsi-ld://test.andromeda/{namespace}/Device:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    // The model refuses it (a Context Source Property), so `a` is written as text.
    let nested_a: RawJson = r#"{"type": "Property", "value": 1, "b": {"type": "Property", "value": 2}}"#.parse()?;
    let nested_document = UncheckedDocument::try_from(nested.render())?.set_member("a", &nested_a);
    let nested_request = core_context_list_body
        .unchecked(&RegisterContextSource { registration: &nested })?
        .document(&nested_document);
    let nested_answer = core_context_list_body.send_unchecked(nested_request).await;
    expect_that!(&nested_answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _nested_cleanup = Cleanup::of(&core_context_list_body).registration(&nested_id);
    let flat = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(flat_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone())
                .with_id_pattern(format!("urn:ngsi-ld://test.andromeda/{namespace}/Device:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    // The model refuses it (a Context Source Property), so `a` is written as text.
    let flat_a: RawJson = r#"{"type": "Property", "value": 1}"#.parse()?;
    let flat_document = UncheckedDocument::try_from(flat.render())?.set_member("a", &flat_a);
    let flat_request = core_context_list_body
        .unchecked(&RegisterContextSource { registration: &flat })?
        .document(&flat_document);
    let flat_answer = core_context_list_body.send_unchecked(flat_request).await;
    expect_that!(&flat_answer, answered(StatusCode::CREATED));
    let _flat_cleanup = Cleanup::of(&core_context_list_body).registration(&flat_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(device_type.as_str())
        .csf(q::attr("a").sub_attr("b").exists())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&nested_id)));
    Ok(())
}
