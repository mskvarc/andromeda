//! A post query conjunction without parentheses is bad request data.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem, wire::request_to};
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
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::scope_query::ScopeQuery,
};

/// Pins: Query Entities by POST with `scopeQ` /Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC is refused
/// with `BadRequestData`, and the same conjunction in parentheses selects the entity holding both scopes
/// Clause: 4.19, 5.2.23, 5.7.2.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_scp_4001_a_post_query_conjunction_without_parentheses_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .scope("/Madrid/Gardens/ParqueNorte")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .scope(["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"])
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 4.19: "For logical and grouping parenthesis are needed"; OrScopeQ derives a conjunction only as "(" ScopeQ *(andOp ScopeQ) ")".
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"scopeQ\": \"/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC\"\n}}")));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .scope_q("(/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC)".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    Ok(())
}
