//! A bare date time operand carrying an offset is refused.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, ErrorType, NgsiLdMediaType, Time, Value},
    query::q,
};

/// Pins: Query Entities with q comparing a typed `readingInstant` to a bare `DateTime` carrying a numeric offset
/// answers 400 `BadRequestData`, to the same text quoted selects nothing, and the untyped `readingLabel` equal to
/// the quoted offset-bearing text selects the Entity
/// Clause: 4.6.3, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9037_a_bare_date_time_operand_carrying_an_offset_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_instant_offset_id = namespace.urn("Observation", "instant-offset")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_instant_offset = Entity::builder()
        .id(observation_instant_offset_id.clone())
        .entity_type(observation_type)
        .property("readingTime", Value::from("12:00:00.500000Z".parse::<Time>()?))
        .property("readingInstant", Value::from("2024-01-01T00:00:00.250000Z".parse::<DateTime>()?))
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation_instant_offset).send().await?.done()?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3AObservation%3A{namespace}-instant-offset&q=readingInstant%3D%3D2024-01-01T01%3A00%3A00%2B01%3A00"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3AObservation%3A{namespace}-instant-offset&q=readingInstant%3E2023-12-31T19%3A00%3A00-05%3A00"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?id=urn%3Angsi-ld%3AObservation%3A{namespace}-instant-offset&q=readingInstant%3D%3D2024-01-01T00%3A00%3A00%2B00%3A00"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_offset_id.clone()])
        .q(q::attr("readingInstant").eq("2024-01-01T01:00:00+01:00"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_offset_id.clone()])
        .q(q::attr("readingInstant").eq("2024-01-01T00:00:00+00:00"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![observation_instant_offset_id.clone()])
        .q(q::attr("readingLabel").eq("2024-01-01T01:00:00+01:00"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    Ok(())
}
