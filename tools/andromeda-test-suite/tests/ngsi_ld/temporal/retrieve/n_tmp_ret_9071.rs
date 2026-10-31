//! A malformed entity map header is bad request data before the entity is looked up.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// Pins: Retrieve Temporal Evolution of an Entity with an `NGSILD-EntityMap` header that is not a URI answers 400
/// `BadRequestData` naming the header, for a held Entity and for an unknown one alike
/// Clause: 5.5.9.3, 6.19.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9071_a_malformed_entity_map_header_is_bad_request_data_before_the_entity_is_looked_up() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property(
            "temperature",
            Property::builder(Value::Number("30.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_evolution).send().await?.created()?;
    // The client cannot write this NGSILD-EntityMap header, so the read is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities/{building_id}"),
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
        &format!("temporal/entities/urn:ngsi-ld:Building:{namespace}-absent"),
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
