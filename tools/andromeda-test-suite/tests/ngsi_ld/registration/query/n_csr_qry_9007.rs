//! A query naming only entity ids is too wide.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with an id list and no type, attrs, q or `geoQ` answers 400
/// `BadRequestData`, and the same id list with a type answers 200 with the registrations covering it
/// Clause: 5.10.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9007_a_query_naming_only_entity_ids_is_too_wide() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_breadth_a_id = namespace.registration_id("breadth-a")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_breadth_b_id = namespace.registration_id("breadth-b")?;
    let building_breadth_1_id = namespace.urn("Building", "breadth-1")?;
    let building_breadth_2_id = namespace.urn("Building", "breadth-2")?;
    let registration_breadth_a = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_breadth_a_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_breadth_a).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_breadth_a_cleanup = Cleanup::of(&broker).registration(&registration_breadth_a_id);
    let registration_breadth_b = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_breadth_b_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration_breadth_b).send().await?;
    let _registration_breadth_b_cleanup = Cleanup::of(&broker).registration(&registration_breadth_b_id);
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("csourceRegistrations?id=urn%3Angsi-ld%3ABuilding%3A{namespace}-breadth-1%2Curn%3Angsi-ld%3ABuilding%3A{namespace}-breadth-2"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .ids(vec![building_breadth_1_id, building_breadth_2_id])
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    broker.registrations().delete(&registration_breadth_a_id).send().await?;
    broker.registrations().delete(&registration_breadth_b_id).send().await?;
    Ok(())
}
