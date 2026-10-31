//! Entity map creation carries the too wide rule.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, received_status::ExpectedStatus, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        Property,
        geometry::{Position, Shape},
    },
    query::{Locality, geoquery, q},
};

/// Pins: Create `EntityMap` for Query Entities answers 201 when narrowed by only a type, attrs, q, a geo-query or
/// local=true, and 400 `BadRequestData` (too wide) with only id, `idPattern`, `scopeQ` or id with `idPattern`
/// Clause: 5.14.4.4, 5.5.13
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qen_9007_entity_map_creation_carries_the_too_wide_rule() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vehicle_map_narrowed_id = namespace.urn("Vehicle", "map-narrowed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_map_narrowed = Entity::builder()
        .id(vehicle_map_narrowed_id)
        .entity_type(vehicle_type.clone())
        .property(
            namespace.attribute("speed")?,
            Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle_map_narrowed).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map1_id = &created.body().id;
    broker.entity_maps().delete(map1_id).send().await?;
    let created_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .attrs(vec![format!("speed_{namespace}").parse()?])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map2_id = &created_2.body().id;
    broker.entity_maps().delete(map2_id).send().await?;
    let created_3 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .q(q::attr(&namespace.attribute("speed")?).gt(10))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map3_id = &created_3.body().id;
    broker.entity_maps().delete(map3_id).send().await?;
    let created_4 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_new(1.0, 2.0)?)))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map4_id = &created_4.body().id;
    broker.entity_maps().delete(map4_id).send().await?;
    let created_5 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .locality(Locality::Local)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map5_id = &created_5.body().id;
    broker.entity_maps().delete(map5_id).send().await?;
    // The client refuses the unbounded creation before sending it, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entityMaps?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-too-wide"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client refuses the unbounded creation before sending it, so the request is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entityMaps?idPattern=urn%3Angsi-ld%3AVehicle%3A{namespace}-.%2A"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client refuses the unbounded creation before sending it, so the request is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "entityMaps?scopeQ=%2FMadrid",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client refuses the unbounded creation before sending it, so the request is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entityMaps?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-too-wide&idPattern=urn%3Angsi-ld%3AVehicle%3A{namespace}-.%2A"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
