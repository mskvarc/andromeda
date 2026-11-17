//! A follow up under another context reference is bad request data.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    received_status::ExpectedStatus,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{Context, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Query Entities by type under an `EntityMap` that Create `EntityMap` for Query Entities created under the
/// `@context` served at one URL answers the mapped Entity under that `@context`, through GET and through POST, and
/// answers 400 `BadRequestData` under the `@context` served at another URL defining the same term
/// Clause: 5.5.14, 5.5.9.1, 5.5.9.3
/// Recorded: clause 5.5.14 holds a request under an `EntityMap` to the parameters of the request that created it, and
/// clause 5.5.9.1 answers a different `@context` during pagination with `BadRequestData`; the `@context` is compared
/// as the request references it, so another reference is refused even when it expands every name alike
#[gtest]
#[tokio::test]
async fn n_map_qen_9021_a_follow_up_under_another_context_reference_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let document = format!(r#"{{"@context": {{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle"}}}}"#);
    contexts.serve("contexts/original", &document, Some("application/ld+json")).await?;
    contexts.serve("contexts/mirror", &document, Some("application/ld+json")).await?;
    let original = broker
        .with_context(Context::Single(contexts.reference("contexts/original")?))?
        .with_accept(Accept::new(NgsiLdMediaType::Json));
    let mirror = broker
        .with_context(Context::Single(contexts.reference("contexts/mirror")?))?
        .with_accept(Accept::new(NgsiLdMediaType::Json));
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    original.entities().create(&vehicle).send().await?.done()?;
    let created = original
        .with_heads()
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let entity_map_request = EntityMapRequest::reuse(created.entity_map().valid().or_fail()?.clone());
    let through_get = original
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(through_get.items(), elements_are![has_id(&vehicle_id)]);
    let through_post = original
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(through_post.items(), elements_are![has_id(&vehicle_id)]);
    let answer = mirror
        .with_responses()
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    broker.entity_maps().delete(&created.body().id).send().await?;
    Ok(())
}
