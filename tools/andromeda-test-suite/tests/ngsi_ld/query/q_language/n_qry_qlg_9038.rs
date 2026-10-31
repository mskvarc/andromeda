//! A quoted operand no temporal form admits finds its value.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with q naming five Property values that only resemble a `DateTime`, Time or Date selects
/// the Entity for == of the quoted value, nothing for != of it, and answers 400 `BadRequestData` for the bare value
/// Clause: 4.6.3, 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9038_a_quoted_operand_no_temporal_form_admits_finds_its_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_quoted_text_id = namespace.urn("Observation", "quoted-text")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_quoted_text = Entity::builder()
        .id(observation_quoted_text_id.clone())
        .entity_type(observation_type)
        .property("reading0", "2024-01-01T01:00:00+01:00")
        .property("reading1", "2024-01-01T00:00:00.1234567Z")
        .property("reading2", "12:34:5678")
        .property("reading3", "1234-56-78")
        .property("reading4", "0000-00-00")
        .build()?;
    broker.entities().create(&observation_quoted_text).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading0").eq("2024-01-01T01:00:00+01:00"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading0").ne("2024-01-01T01:00:00+01:00"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3AObservation%3A{namespace}-quoted-text&q=reading0%3D%3D2024-01-01T01%3A00%3A00%2B01%3A00"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading1").eq("2024-01-01T00:00:00.1234567Z"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading1").ne("2024-01-01T00:00:00.1234567Z"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    // The model refuses this q, so the query is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3AObservation%3A{namespace}-quoted-text&q=reading1%3D%3D2024-01-01T00%3A00%3A00.1234567Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading2").eq("12:34:5678"))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading2").ne("12:34:5678"))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(0)));
    // The model refuses this q, so the query is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3AObservation%3A{namespace}-quoted-text&q=reading2%3D%3D12%3A34%3A5678"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading3").eq("1234-56-78"))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading3").ne("1234-56-78"))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(0)));
    // The model refuses this q, so the query is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3AObservation%3A{namespace}-quoted-text&q=reading3%3D%3D1234-56-78"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading4").eq("0000-00-00"))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(1)));
    let page_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_quoted_text_id.clone()])
        .q(q::attr("reading4").ne("0000-00-00"))
        .send()
        .await?;
    expect_that!(page_10.items(), len(eq(0)));
    // The model refuses this q, so the query is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3AObservation%3A{namespace}-quoted-text&q=reading4%3D%3D0000-00-00"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
