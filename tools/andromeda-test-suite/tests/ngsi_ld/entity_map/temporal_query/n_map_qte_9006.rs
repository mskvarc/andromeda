//! A posted temporal map creation refuses url parameters.

use andromeda_test_suite::{
    broker,
    header_matcher::present,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    received_status::ExpectedStatus,
    wire::api_uri,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::operation::{CreateTemporalEntityMap, QueryMethod},
    model::{ErrorType, Property, Value, evolution::TemporalEntity},
    query::{
        builders::SelectorTypeBuilder,
        temporal_query::{self, TemporalQuery},
    },
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities via POST with limit=3 on the URL answers 400
/// `InvalidRequest` without an `NGSILD-EntityMap` header, while the same body without URL parameters answers 201
/// with one
/// Clause: 6.3.20, Table 6.35.3.2-1, 5.14.5
#[gtest]
#[tokio::test]
async fn n_map_qte_9006_a_posted_temporal_map_creation_refuses_url_parameters() -> Result<()> {
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
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let before = TemporalQuery::before("2099-01-01T00:00:00Z".parse()?);
    let creation = CreateTemporalEntityMap::builder()
        .entity_type(&selector)
        .temporal_q(&before)
        .method(QueryMethod::Post)
        .build();
    // The operation writes no URL parameter on the POST binding, so it is sent to the path as written.
    let request = broker.unchecked(&creation)?.uri(api_uri(&broker, "temporal/entityMaps?limit=3")?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    let created = broker
        .with_heads()
        .entity_maps()
        .create_temporal(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(created.entity_map(), present());
    broker.entity_maps().delete(&created.body().id).send().await?;
    Ok(())
}
