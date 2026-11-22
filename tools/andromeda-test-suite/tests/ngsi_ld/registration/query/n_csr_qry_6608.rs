//! Pick and omit leave the whole registration in the answer.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    raw_call::target,
    unchecked_call::exchange,
    unchecked_outcome::read_as,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::QueryRegistrations,
    query::type_selection::TypeSelection,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};
use std::iter::once;

/// Pins: Query Context Source Registrations by type with pick endpoint, and with omit endpoint, answers 200 with the
/// whole registration (id, type, endpoint and information), as the same query without them does
/// Clause: 5.10.2.3, 5.10.2.4, 6.8.3.2
#[gtest]
#[tokio::test]
async fn n_csr_qry_6608_pick_and_omit_leave_the_whole_registration_in_the_answer() -> Result<()> {
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
    broker.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let type_text = vehicle_type.to_string();
    let vehicles = type_text.parse::<TypeSelection>()?;
    let discovery = QueryRegistrations::builder().entity_type(&vehicles).build();
    // The typed client has no pick or omit on registrations, so the query string is written as given, and the answer
    // is read through the discovery of the same type, which holds each registration to its mandatory members.
    for projection in [None, Some(("pick", "endpoint")), Some(("omit", "endpoint"))] {
        let parameters: Vec<(&str, &str)> = once(("type", type_text.as_str())).chain(projection).collect();
        let request = exchange(
            &broker,
            Method::GET,
            &target("csourceRegistrations", &parameters),
            &[("Accept", "application/json")],
            None,
        )?;
        let sent = request.as_request().clone();
        let response = broker.send_unchecked(request).await?;
        expect_that!(response.status(), eq(StatusCode::OK), "{projection:?}");
        let page = read_as(&discovery, &sent, response)?;
        let registrations = page.items();
        expect_that!(registrations, len(eq(1)), "{projection:?}");
        expect_that!(registrations, nth(0, has_id(&registration_id)), "{projection:?}");
        let Some(found) = registrations.first() else {
            return fail!("{projection:?} answered no registration");
        };
        expect_that!(found.endpoint(), eq(config.unreachable_origin()), "{projection:?}");
        expect_that!(found.information(), len(eq(1)), "{projection:?}");
        expect_that!(found.information().first().map(|info| info.entities().len()), some(eq(1)), "{projection:?}");
    }
    Ok(())
}
