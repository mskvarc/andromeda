//! A malformed entity map header is bad request data and creates no map.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// Pins: Query Temporal Evolution of Entities with `entityMap` true and an `NGSILD-EntityMap` header that is not a
/// URI answers 400 `BadRequestData` naming the header and creates no `EntityMap`
/// Clause: 5.5.9.3, 5.14.5
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9075_a_malformed_entity_map_header_is_bad_request_data_and_creates_no_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id)
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
        &format!("temporal/entities?type=Building_{namespace}&timerel=before&timeAt=2099-01-01T00%3A00%3A00Z&entityMap=true"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(HeaderName::from_static("ngsild-entitymap"), HeaderValue::from_static("not-a-uri"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail("the ngsild-entitymap header is not a uri")
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    Ok(())
}
