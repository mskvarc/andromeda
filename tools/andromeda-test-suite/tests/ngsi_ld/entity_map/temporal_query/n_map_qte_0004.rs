//! A temporal query under an entity map echoes it.

use andromeda_test_suite::{
    broker,
    header_matcher::present,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Upserted,
    received_status::ExpectedStatus,
    unchecked_outcome::read_as,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryTemporalEvolution},
    },
    model::{Lossless, Property, evolution::TemporalEntity},
    query::{
        builders::SelectorTypeBuilder,
        temporal_query::{self, TemporalQuery},
    },
};

/// Pins: Query Temporal Evolution of Entities by type naming an `EntityMap` in `NGSILD-EntityMap` answers 200 with
/// the same `NGSILD-EntityMap` header
/// Clause: 5.5.14, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_map_qte_0004_a_temporal_query_under_an_entity_map_echoes_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2024-01-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_map(&entity_map_request)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map = page.entity_map().valid().or_fail()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let before = TemporalQuery::before("2030-01-01T00:00:00Z".parse()?);
    let query = QueryTemporalEvolution::builder()
        .entity_type(&selector)
        .temporal_q(&before)
        .entity_map(&entity_map_request_2)
        .format(Lossless::Normalized)
        .build();
    let client = broker.with_accept(Accept::any());
    // The operation writes no query without its temporal query, so it is sent to the path as written.
    let request = client
        .unchecked(&query)?
        .uri(api_uri(&client, &format!("temporal/entities?type=Vehicle_{namespace}"))?);
    let sent = request.as_request().clone();
    let response = client.send_unchecked(request).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    let answered = read_as(&query, &sent, response)?;
    expect_that!(answered.entity_map().valid(), some(eq(map)));
    expect_that!(answered.items(), len(eq(1)));
    expect_that!(answered.items(), contains(has_id(&vehicle_id)));
    Ok(())
}
