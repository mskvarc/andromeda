//! A bare word for a sub attribute or a trailing path is bad request data.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    value::object,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property, Value},
    query::q,
};

/// Pins: a q value written as an unquoted bare word is refused with `BadRequestData` when the target is a
/// sub-attribute (reading.scale==CEL) or a trailing path member (reading`[unit]`==CEL), and `expandValues` naming
/// another attribute does not admit it
/// Clause: 4.9, 5.7.2.4, 6.4.3.2, 6.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4010_a_bare_word_for_a_sub_attribute_or_a_trailing_path_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let person_bob_id = namespace.urn("Person", "bob")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("reading", Property::builder(object([("unit", Value::from("CEL"))])).property("scale", "CEL"))
        .relationship("owner", person_bob_id)
        .build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=reading.scale%3D%3DCEL"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=reading%5Bunit%5D%3D%3DCEL"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Device_{namespace}&q=reading.scale%3D%3DCEL&expandValues=owner"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The quoted forms are the String.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .q(q::attr("reading").sub_attr("scale").eq("CEL"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&device_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .q(q::attr("reading").member("unit").eq("CEL"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&device_id)));
    Ok(())
}
