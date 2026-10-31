//! Entity map lifetime without entity map is invalid request.

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
    http::{headers::Accept, operation::EntityMapRequest},
    model::{ErrorType, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with `entityMapLifetime` PT1H and no `entityMap` answers 400
/// `InvalidRequest` without an `NGSILD-EntityMap` header, while `entityMap` true with the lifetime answers 201 with
/// one
/// Clause: 6.3.20, Table 6.18.3.2-1, 5.14.5
#[gtest]
#[tokio::test]
async fn n_map_qte_9007_entity_map_lifetime_without_entity_map_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=before&timeAt=2099-01-01T00%3A00%3A00Z&entityMapLifetime=PT1H"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
            .with_detail_containing("entityMapLifetime")
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    let entity_map_request = EntityMapRequest::returned_for("PT1H".parse()?);
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_map(&entity_map_request)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    Ok(())
}
