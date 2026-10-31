//! A csf path reads a compound context source property by its written key.

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

/// Pins: Query Context Source Registrations by type and a csf naming a Context Source Property with a trailing
/// `[region]` path answers 200 with only the registration whose property value holds that region
/// Clause: 5.10.2, 4.9, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_qry_0013_a_csf_path_reads_a_compound_context_source_property_by_its_written_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities(
                [EntityInfo::new(format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?)
                    .with_id_pattern(format!("urn:ngsi-ld://test.andromeda/{namespace}/Device:{namespace}-.*").parse::<IdPattern>()?)],
            )
            .build()?])
        .build()?;
    // The model refuses it (a registration a registration property), so `coverage` is written as text.
    let coverage: RawJson = r#"{"type": "Property", "value": {"region": "north"}}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("coverage", &coverage);
    let request = core_context_list_body
        .unchecked(&RegisterContextSource { registration: &registration })?
        .document(&document);
    let answer = core_context_list_body.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&core_context_list_body).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities(
                [EntityInfo::new(format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?)
                    .with_id_pattern(format!("urn:ngsi-ld://test.andromeda/{namespace}/Device:{namespace}-.*").parse::<IdPattern>()?)],
            )
            .build()?])
        .build()?;
    // The model refuses it (a registration a registration property), so `coverage` is written as text.
    let coverage_2: RawJson = r#"{"type": "Property", "value": {"region": "south"}}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("coverage", &coverage_2);
    let request_2 = core_context_list_body
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = core_context_list_body.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_2_cleanup = Cleanup::of(&core_context_list_body).registration(&registration_2_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .csf(q::attr("coverage").member("region").eq("north"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .csf(q::attr("coverage").member("region").eq("south"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_2_id)));
    Ok(())
}
