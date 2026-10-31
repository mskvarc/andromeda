//! A post temporal query scope conjunction without parentheses is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{ErrorType, NgsiLdMediaType},
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities by POST refuses with `BadRequestData` a `scopeQ` member that joins
/// scopes with ; outside parentheses
/// Clause: 4.19, 5.2.23, 5.7.4.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_4003_a_post_temporal_query_scope_conjunction_without_parentheses_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"scopeQ\": \"/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC\",\n    \"temporalQ\": {{\n        \"timerel\": \"after\",\n        \"timeAt\": \"2020-07-01T00:00:00Z\"\n    }}\n}}")));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&namespace.entity_type("Building")?)
        .scope_q("(/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC)".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
