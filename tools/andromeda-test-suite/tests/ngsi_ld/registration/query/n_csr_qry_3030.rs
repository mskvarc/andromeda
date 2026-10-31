//! Pick or omit with no narrowing input is too wide.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_registration::entity_registration,
    namespace::Namespace,
    problem::problem,
    raw_call::{call, target},
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Context Source Registrations whose only restriction is a pick or an omit (an id list or an
/// `idPattern` at most beside it) is too wide and refused with `BadRequestData`, and the same pick beside a type
/// selector is answered
/// Clause: 5.10.2.4, 6.8.3.2, 4.21
#[gtest]
#[tokio::test]
async fn n_csr_qry_3030_pick_or_omit_with_no_narrowing_input_is_too_wide() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = entity_registration(
        &registration_id,
        &building_type,
        namespace.id_pattern("Building")?,
        source.endpoint("1")?.as_str(),
    )?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // Query Context Source Registrations has no `pick` or `omit` parameter, so the query string is written as given.
    let id_pattern = format!("urn:ngsi-ld:Building:{namespace}-.*");
    let type_text = building_type.to_string();
    for parameters in [
        vec![("pick", "p")],
        vec![("omit", "p")],
        vec![("idPattern", id_pattern.as_str()), ("pick", "p")],
    ] {
        let request = exchange(
            &broker,
            Method::GET,
            &target("csourceRegistrations", &parameters),
            &[("Accept", "application/json")],
            None,
        )?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    // A `pick` of `p` leaves a registration without `information` or `endpoint`, which the model cannot read, so the
    // page is read as received.
    let response = call(
        &broker,
        Method::GET,
        &target("csourceRegistrations", &[("type", type_text.as_str()), ("pick", "p")]),
        &[("Accept", "application/json")],
        None,
    )
    .await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(response.body(), json_count("$", 1));
    expect_that!(response.body(), json_equals("$[0].id", registration_id.to_string()));
    Ok(())
}
