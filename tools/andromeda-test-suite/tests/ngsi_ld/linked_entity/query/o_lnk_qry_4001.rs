//! A non integer join level is invalid request under both joins.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with a `joinLevel` that is not an integer (1.5, abc, 1e1, empty) is refused with
/// `InvalidRequest` under join=inline and join=flat, and `joinLevel=1` is answered
/// Clause: 6.4.3.2, 4.5.23, 5.5.2
/// Recorded: Table 5.5.2-1 defines `InvalidRequest` as a request that is syntactically invalid, which fits a
/// `joinLevel` that is no integer at all (1.5, abc, 1e1 and the empty value, which is no integer either);
/// `BadRequestData` is for an integer below 1. Ruled by the user (2026-10-05)
#[gtest]
#[tokio::test]
async fn o_lnk_qry_4001_a_non_integer_join_level_is_invalid_request_under_both_joins() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_type = namespace.entity_type("Building")?;
    let building_a = Entity::builder().id(building_a_id).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_a).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&join=inline&joinLevel=1.5"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&join=inline&joinLevel=abc"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&join=flat&joinLevel=1e1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&join=flat&joinLevel="),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
