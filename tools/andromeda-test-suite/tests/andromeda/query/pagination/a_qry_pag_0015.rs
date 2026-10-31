//! A malformed cursor is bad request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities with a cursor the broker never issued answers 400
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn a_qry_pag_0015_a_malformed_cursor_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0000 = Entity::builder()
        .id(building_0000_id)
        .entity_type(building_type)
        .property("name", "b0")
        .build()?;
    broker.entities().create(&building_0000).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&local=true&limit=2&cursor=zzzz"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    Ok(())
}
