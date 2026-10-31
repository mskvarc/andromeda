//! A registration query refuses a dataset id.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type with a `datasetId` answers 400 `InvalidRequest`, while the same
/// query without it answers 200 with the registration listing that `datasetId`
/// Clause: 6.3.20, Table 6.8.3.2-1, 5.10.2.3
/// Recorded: Table 6.8.3.2-1 defines no `datasetId`; the clause 5.12 dataset rule belongs to the matching of
/// distributed operations, not to discovery
#[gtest]
#[tokio::test]
async fn o_csr_mat_6602_a_registration_query_refuses_a_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration datasetId), so `datasetId` is written as text.
    let dataset_id: RawJson = format!(r#"["urn:ngsi-ld:Dataset:{namespace}-shared"]"#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("datasetId", &dataset_id);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("csourceRegistrations?type=Vehicle_{namespace}&datasetId=urn%3Angsi-ld%3ADataset%3A{namespace}-shared"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
