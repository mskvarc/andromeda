//! The temporal operations refuse linked entity retrieval.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Temporal Evolution with pick owner{name}, Query Temporal Evolution with q owner{name}=="x", and
/// the temporal POST query with join flat answer 400 `BadRequestData`, and join on Retrieve Temporal Evolution
/// answers 400 `InvalidRequest`
/// Clause: 5.7.3.4, 5.7.4.4, 6.3.20, Table 6.18.3.1-1
#[gtest]
#[tokio::test]
async fn n_lnk_tmp_9001_the_temporal_operations_refuse_linked_entity_retrieval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let company_id = namespace.urn("Company", "r8")?;
    let company_type = namespace.entity_type("Company")?;
    let person_id = namespace.urn("Person", "r8")?;
    let person_type = namespace.entity_type("Person")?;
    let city_id = namespace.urn("City", "r8")?;
    let city_type = namespace.entity_type("City")?;
    let building_id = namespace.urn("Building", "r8")?;
    let building_type = namespace.entity_type("Building")?;
    let company = Entity::builder()
        .id(company_id.clone())
        .entity_type(company_type)
        .property("name", "Acme")
        .property("size", 50)
        .build()?;
    broker.entities().create(&company).send().await?.done()?;
    let person = Entity::builder()
        .id(person_id.clone())
        .entity_type(person_type)
        .property("name", "Ada")
        .property("email", "ada@example.com")
        .relationship("employer", company_id.clone())
        .build()?;
    broker.entities().create(&person).send().await?.done()?;
    let city = Entity::builder().id(city_id.clone()).entity_type(city_type).property("name", "Paris").build()?;
    broker.entities().create(&city).send().await?.done()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .relationship("owner", person_id.clone())
        .relationship("tenant", person_id.clone())
        .relationship("locatedIn", city_id.clone())
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The temporal operations define no linked-entity term, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities/{building_id}?pick=owner%7Bname%7D&timeproperty=modifiedAt&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The temporal operations define no linked-entity term, so the request is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "temporal/entities?type=Building_{namespace}&q=owner%7Bname%7D%3D%3D%22x%22&timeproperty=modifiedAt&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The temporal query builders have no join, so the Query document is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"id\": \"{building_id}\",\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"join\": \"flat\",\n    \"temporalQ\": {{\n        \"timeproperty\": \"modifiedAt\",\n        \"timerel\": \"after\",\n        \"timeAt\": \"2000-01-01T00:00:00Z\"\n    }}\n}}"
        )));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The temporal operations define no join, so the request is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities/{building_id}?join=flat&timeproperty=modifiedAt&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
