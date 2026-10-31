//! A boolean after any ordering operator is bad request data.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by GET refuses a q whose ordering operator (>, >=, <, <=) is followed by true or false with
/// `BadRequestData`, for each operator and each Boolean, and answers the same Booleans after == and !=
/// Clause: 4.9, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_3001_a_boolean_after_any_ordering_operator_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("b", true)
        .property("p", 1)
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    // QueryTerm =/ Attribute Operator ComparableValue, ComparableValue = Number / quotedStr / dateTime / date / time; true and false are OtherValue, admitted only as CompEqualityValue after == and !=
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=b%3Etrue"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=b%3Efalse"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=b%3E%3Dtrue"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=b%3E%3Dfalse"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=b%3Ctrue"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=b%3Cfalse"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_7 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=b%3C%3Dtrue"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_8 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=b%3C%3Dfalse"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_8 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_8).await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a Boolean as the later operand of a value list after an ordering operator is just as outside ComparableValue
    // The model refuses this q, so the query is sent as written.
    let request_9 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=p%3E1%2Ctrue"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_9 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_9).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a Boolean term inside a combination is refused as well, and refuses the whole query
    // The model refuses this q, so the query is sent as written.
    let request_10 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=p%3D%3D1%3Bb%3Etrue"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_10 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_10).await;
    expect_that!(&answer_10, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_11 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Sensor_{namespace}&q=%28p%3D%3D1%7Cb%3Cfalse%29"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_11 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_11).await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the same values are well-formed after == and !=
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("b").eq(true))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("b").ne(false))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&sensor_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("p").any_of([q::Value::from(q::ComparableValue::from(1)), q::Value::from(true)]))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
