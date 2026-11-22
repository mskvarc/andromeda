//! A bare any-scope wildcard beside another scope is bad request data.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::scope_query::ScopeQuery,
};

/// Pins: Query Entities refuses with `BadRequestData` each `scopeQ` that holds the bare `/#` inside a group or beside
/// another alternative, and answers `/#` alone with the Entity that holds a Scope and not the one that holds none
/// Clause: 4.19, 5.7.2.4
/// Recorded: clause 4.19 writes the bare `/#` only as the whole expression, `ScopesQ =/ %x2F %23`
#[gtest]
#[tokio::test]
async fn n_qry_scp_4004_a_bare_any_scope_wildcard_beside_another_scope_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let scoped_id = namespace.urn("Building", "scoped")?;
    let entities_written = [
        Entity::builder()
            .id(scoped_id.clone())
            .entity_type(building_type.clone())
            .scope("/CompanyA")
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", "unscoped")?)
            .entity_type(building_type.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities_written).send().await?.done()?;
    let type_text = building_type.to_string();
    for scope_q in ["(/#;/CompanyA)", "(/CompanyA;/#)", "(/#)", "/#,/CompanyA", "/CompanyA|/#", "/#,/#"] {
        // The client cannot write a parameter of this read, so it is sent as written.
        let read = target("entities", &[("type", type_text.as_str()), ("scopeQ", scope_q)]);
        let answer = broker
            .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
            .await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "scopeQ {scope_q}");
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&scoped_id)));
    Ok(())
}
