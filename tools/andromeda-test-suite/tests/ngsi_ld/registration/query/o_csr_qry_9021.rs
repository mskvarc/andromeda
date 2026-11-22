//! Attrs beside pick or omit is bad request data.

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

/// Pins: Query Context Source Registrations answers 400 `BadRequestData` for attrs beside pick, beside omit and
/// beside both, with or without a type selector, and the same attrs beside the type selector alone answers the
/// registration
/// Clause: 5.10.2.4, 6.8.3.2
/// Recorded: no clause forbids combining pick or omit with the deprecated attrs, which Table 6.8.3.2-1 makes "A
/// synonym for a combination of the pick and q members", as Table 6.4.3.2-1 does for Query Entities; Andromeda
/// refuses attrs beside pick or omit with `BadRequestData` on every operation that takes them
#[gtest]
#[tokio::test]
async fn o_csr_qry_9021_attrs_beside_pick_or_omit_is_bad_request_data() -> Result<()> {
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
    // The typed client has no `pick` or `omit` on registrations, so the query string is written as given.
    let type_text = building_type.to_string();
    let height = namespace.attribute("height")?.to_string();
    for parameters in [
        vec![("attrs", height.as_str()), ("pick", "id")],
        vec![("attrs", height.as_str()), ("omit", "endpoint")],
        vec![("type", type_text.as_str()), ("attrs", height.as_str()), ("pick", "id")],
        vec![("type", type_text.as_str()), ("attrs", height.as_str()), ("omit", "endpoint")],
        vec![("type", type_text.as_str()), ("attrs", height.as_str()), ("pick", "id"), ("omit", "endpoint")],
    ] {
        let request = exchange(
            &broker,
            Method::GET,
            &target("csourceRegistrations", &parameters),
            &[("Accept", "application/json")],
            None,
        )?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title(),
            "{parameters:?}"
        );
    }
    // Clause 5.10.2.4: a registration that names no Attributes matches every Attribute name.
    let response = call(
        &broker,
        Method::GET,
        &target("csourceRegistrations", &[("type", type_text.as_str()), ("attrs", height.as_str())]),
        &[("Accept", "application/json")],
        None,
    )
    .await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(response.body(), json_count("$", 1));
    expect_that!(response.body(), json_equals("$[0].id", registration_id.to_string()));
    Ok(())
}
