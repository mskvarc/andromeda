//! A temporal query of every type refuses local false.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities and Create `EntityMap` for it with type * and local false answer 400
/// `InvalidRequest`, while type * alone answers 200 with the entity
/// Clause: 6.3.20, Table 6.18.3.2-1, 6.35.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9082_a_temporal_query_of_every_type_refuses_local_false() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The client refuses every type with local=false before sending, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=%2A&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&local=false&timerel=before&timeAt=2099-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The client refuses every type with local=false before sending, so the request is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entityMaps?type=%2A&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&local=false&timerel=before&timeAt=2099-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_type(SelectorTypeBuilder::All)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    Ok(())
}
