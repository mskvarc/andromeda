//! A scope conjunction without parentheses is bad request data.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::{answered, problem},
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, Lossless, NgsiLdMediaType, Property, RawJson, evolution::TemporalEntity},
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities refuses with `BadRequestData` a `scopeQ` that joins scopes with ;
/// outside parentheses, and answers the parenthesised conjunction
/// Clause: 4.19, 5.7.4.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_4002_a_scope_conjunction_without_parentheses_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", Property::builder("Depot").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    // The model refuses it (a temporal scope), so `scope` is written as text.
    let scope: RawJson =
        r#"[{"type": "Property", "value": ["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"], "observedAt": "2020-08-01T12:03:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(building_evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&building_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=Building_{namespace}&scopeQ=%2FMadrid%2FGardens%2FParqueNorte%3B%2FCompanyA%2FOrganizationB%2FUnitC&timerel=after&timeAt=2020-07-01T00%3A00%3A00Z"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=Building_{namespace}&scopeQ=%2FOther%2C%2FMadrid%2FGardens%2FParqueNorte%3B%2FCompanyA%2FOrganizationB%2FUnitC&timerel=after&timeAt=2020-07-01T00%3A00%3A00Z"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .scope_q("(/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC)".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    Ok(())
}
