//! A malformed entity map header is bad request data before the entity is looked up.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with an `NGSILD-EntityMap` header that is not a URI answers 400 `BadRequestData` naming
/// the header, for a held Entity and for an unknown one alike
/// Clause: Table 6.5.3.1-2, 5.5.9.3
#[gtest]
#[tokio::test]
async fn n_ent_ret_9038_a_malformed_entity_map_header_is_bad_request_data_before_the_entity_is_looked_up() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The client cannot write this NGSILD-EntityMap header, so the read is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_id}"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(HeaderName::from_static("ngsild-entitymap"), HeaderValue::from_static("not-a-uri"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the ngsild-entitymap header is not a uri")
    );
    // The client cannot write this NGSILD-EntityMap header, so the read is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/urn:ngsi-ld:Building:{namespace}-absent"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(HeaderName::from_static("ngsild-entitymap"), HeaderValue::from_static("not-a-uri"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the ngsild-entitymap header is not a uri")
    );
    Ok(())
}
