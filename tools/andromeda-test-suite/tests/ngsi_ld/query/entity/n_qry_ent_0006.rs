//! Attrs naming only core members is too wide.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: an attrs list naming only id and type, or only system Attributes, is no narrowing input, so a query with
/// nothing else is too wide (400 `BadRequestData`)
/// Clause: 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ent_0006_attrs_naming_only_core_members_is_too_wide() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder().id(building_id).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The rule attrs=id,type hits is the too-wide rule of 5.7.2.4 b), which needs a non-system Attribute
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, "entities?attrs=id%2Ctype")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "entities?attrs=createdAt%2CmodifiedAt",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
