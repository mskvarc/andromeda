//! A posted query under a scoped or unreachable context creates no map.

use andromeda_test_suite::{broker, config::SuiteConfig, core_context::core_entry, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: Create `EntityMap` through POST `/entityMaps` and POST `/temporal/entityMaps` answers 400 `BadRequestData`
/// for an @context declaring a scoped context and 504 `LdContextNotAvailable` for a context Link that cannot be
/// retrieved, and creates no `EntityMap`
/// Clause: 5.5.6, 5.5.7, 6.34.3.2, 6.35.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_0011_a_posted_query_under_a_scoped_or_unreachable_context_creates_no_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"speed": {{"@id": "https://test.andromeda/{namespace}/speed", "@context": {{}}}}}}"#).parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}/contexts/{namespace}/missing.jsonld"
    ))?)))?;
    let entities = [Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let answer = body_context
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .without_header(HeaderName::from_static("ngsild-entitymap"))
            .without_header(HeaderName::from_static("location"))
    );
    let answer_2 = linked
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable)
            .without_header(HeaderName::from_static("ngsild-entitymap"))
            .without_header(HeaderName::from_static("location"))
    );
    let answer_3 = body_context
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .send()
        .await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .without_header(HeaderName::from_static("ngsild-entitymap"))
            .without_header(HeaderName::from_static("location"))
    );
    let answer_4 = linked
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .send()
        .await;
    expect_that!(
        &answer_4,
        problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable)
            .without_header(HeaderName::from_static("ngsild-entitymap"))
            .without_header(HeaderName::from_static("location"))
    );
    Ok(())
}
