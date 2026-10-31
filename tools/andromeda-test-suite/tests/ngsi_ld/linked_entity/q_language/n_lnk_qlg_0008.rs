//! A hop without a retrieving join is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: a q hop without Linked Entity retrieval, with no join or with join=@none (its default), is
/// `BadRequestData`
/// Clause: 5.7.2.4, 4.5.23, 4.9, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0008_a_hop_without_a_retrieving_join_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Car")?)
        .q(q::attr("isParkedIn").linked("category").eq("commercial"))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Car_{namespace}&q=isParkedIn%7Bcategory%7D%3D%3D%22commercial%22&join=%40none"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
