//! A posted map creation refuses url parameters.

use andromeda_test_suite::{
    broker,
    header_matcher::present,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    received_status::ExpectedStatus,
    wire::api_uri,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::operation::{CreateEntityMap, QueryMethod},
    model::{Entity, ErrorType},
    query::builders::SelectorTypeBuilder,
};

/// Pins: Create `EntityMap` for Query Entities via POST with limit=3 or local=true on the URL answers 400
/// `InvalidRequest` without an `NGSILD-EntityMap` header, while the same body without URL parameters answers 201
/// with one
/// Clause: 6.3.20, Table 6.34.3.2-1, 5.14.4
#[gtest]
#[tokio::test]
async fn n_map_qen_9009_a_posted_map_creation_refuses_url_parameters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let creation = CreateEntityMap::builder().entity_type(&selector).method(QueryMethod::Post).build();
    // The operation writes no URL parameter on the POST binding, so it is sent to the path as written.
    let request = broker.unchecked(&creation)?.uri(api_uri(&broker, "entityMaps?limit=3")?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // The operation writes no URL parameter on the POST binding, so it is sent to the path as written.
    let request_2 = broker.unchecked(&creation)?.uri(api_uri(&broker, "entityMaps?local=true")?);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let created = broker
        .with_heads()
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(created.entity_map(), present());
    broker.entity_maps().delete(&created.body().id).send().await?;
    Ok(())
}
