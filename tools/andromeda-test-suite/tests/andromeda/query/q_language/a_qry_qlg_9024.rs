//! An unspecified bracket expression is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Query Entities with a q pattern holding an unterminated class name, a class intersection or a range
/// bounded by a bracket or a hyphen answers 400 `BadRequestData`
/// Clause: 4.9
/// Recorded: IEEE 1003.2 leaves a backslash before an ordinary character, an inline option, a repeated duplication
/// symbol, an undefined class name and a range outside the POSIX locale undefined or unspecified; Andromeda refuses
/// each with `BadRequestData` so that the stored-query and the subscription evaluators never read one pattern two
/// ways
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9024_an_unspecified_bracket_expression_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_dialect_bracket_id = namespace.urn("Building", "dialect-bracket")?;
    let building_type = namespace.entity_type("Building")?;
    let building_dialect_bracket = Entity::builder()
        .id(building_dialect_bracket_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Mercedes")
        .build()?;
    broker.entities().create(&building_dialect_bracket).send().await?.done()?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "entities?type=Building_{namespace}&local=true&id=urn%3Angsi-ld%3ABuilding%3A{namespace}-dialect-bracket&q=name~%3D%22%5B%5B%3Aalpha%5D%5D%22"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_bracket_id.clone()])
        .q(q::attr("name").matches("\"[a-z&&b-d]\""))
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_bracket_id.clone()])
        .q(q::attr("name").matches("\"[]-a]\""))
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_bracket_id.clone()])
        .q(q::attr("name").matches("\"[--/]\""))
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
