//! A cursor beside an order by is bad request data.

use andromeda_test_suite::{
    broker,
    followed_link::request_with,
    link_header::link_text,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::{call, target},
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
    },
    model::{Entity, ErrorType},
};

/// Pins: Query Entities following a next pointer with `orderBy` name added answers 400 `BadRequestData`, a cursor
/// being a position in identifier order
/// Clause: 5.5.9.1, 4.23
#[gtest]
#[tokio::test]
async fn a_qry_pag_0034_a_cursor_beside_an_order_by_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    for number in 0..3 {
        let building = Entity::builder()
            .id(namespace.urn("Building", format!("{number:04}"))?)
            .entity_type(building_type.clone())
            .property("name", format!("b{number}"))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    // `local` is a parameter the query model does not write, so the query is sent as written.
    let building_type_text = building_type.to_string();
    let first = call(
        &broker,
        Method::GET,
        &target("entities", &[("type", building_type_text.as_str()), ("local", "true"), ("limit", "2")]),
        &[("Accept", "application/ld+json")],
        None,
    )
    .await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    expect_that!(link_text(first.headers()), contains_substring("rel=\"next\""));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let followed =
        request_with(&broker, &link, &RelationType::Next, &[("orderBy", "name")])?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.send_unchecked(followed).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
