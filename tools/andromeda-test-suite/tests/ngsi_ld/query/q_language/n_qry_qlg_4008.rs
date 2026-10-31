//! A bare word in a value list a range or a logical combination is bad request data.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: a q holding an unquoted bare word inside a value list, a range bound or one side of an and, an or or a
/// parenthesised expression is refused with `BadRequestData` as a whole, even when the other terms are valid
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 6.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4008_a_bare_word_in_a_value_list_a_range_or_a_logical_combination_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("status", "active")
        .property("level", 5)
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=status%3D%3D%22active%22%2Cidle"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=status%3D%3Dactive%2Cidle"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=status%3D%3Da..z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=level%3E1%3Bstatus%3D%3Dactive"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=level%3E1%7Cstatus%3D%3Dactive"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=%28level%3E1%3Bstatus%3D%3Dactive%29"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .q(q::attr("level").gt(1).and(q::attr("status").eq("active")))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&device_id)));
    Ok(())
}
