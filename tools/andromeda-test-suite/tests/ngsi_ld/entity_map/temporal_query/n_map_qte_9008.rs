//! Temporal map creation passes the temporal extension gate.

use andromeda_test_suite::{
    broker,
    header_matcher::present,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    received_status::ExpectedStatus,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: With the temporal extensions off, Create `EntityMap` for Query Temporal Evolution of Entities over GET
/// with join flat, or with `aggrGroupBy` and format `groupedValues`, answers 400 `InvalidRequest` without an
/// `NGSILD-EntityMap` header, as Query Temporal Evolution of Entities does
/// Clause: 6.3.20, 5.14.5.4, Table 6.18.3.2-1, 6.35.3.1
#[gtest]
#[tokio::test]
async fn n_map_qte_9008_temporal_map_creation_passes_the_temporal_extension_gate() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The client cannot write these parameters on this creation, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entityMaps?type=Vehicle_{namespace}&timerel=before&timeAt=2099-01-01T00%3A00%3A00Z&join=flat"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // The client cannot write these parameters on this creation, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "temporal/entityMaps?type=Vehicle_{namespace}&timerel=before&timeAt=2099-01-01T00%3A00%3A00Z&format=groupedValues&aggrMethods=sum&aggrGroupBy=unitCode"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(created.entity_map(), present());
    Ok(())
}
