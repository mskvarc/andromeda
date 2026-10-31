//! A chain one past the depth limit is refused and the broker keeps serving.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    minted_wrapper::listed_wrapper_ids,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{ContextKindFilter, QueryMethod},
    },
    model::{Context, Entity, EntityFragment, EntityTypeName, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity, Create Entity and Update Attributes under a Link naming the first of nine documents, each
/// naming the next, and Create Entity and Query Entities by POST as application/ld+json naming it in the body, are
/// each refused as `BadRequestData`; the broker keeps answering, nothing is written and no wrapper is hosted, and
/// the ninth document is never fetched
/// Clause: 5.5.6, 6.3.5, JSON-LD 1.1 API s4.1.2
/// Tenant: private (the case asserts what the tenant holds afterwards, and clause 4.14 keeps it the tenant's own)
#[gtest]
#[tokio::test]
async fn a_ctx_exp_0041_a_chain_one_past_the_depth_limit_is_refused_and_the_broker_keeps_serving() -> Result<()> {
    let namespace = Namespace::fresh();
    let config = SuiteConfig::get()?;
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let contexts = RemoteContexts::start().await?;
    let core = config.core_context();
    let served = contexts.url("")?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{"@context": {{"Device_{namespace}": "https://test.andromeda/{namespace}/Device", "speed": "https://test.andromeda/{namespace}/speed"}}}}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/past-limit/9.jsonld",
            &format!(
                r#"{{"@context": [{{"Device_{namespace}": "https://test.andromeda/{namespace}/Device", "speed": "https://test.andromeda/{namespace}/speed"}}, "{core}"]}}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    for level in (1..9).rev() {
        contexts
            .serve(
                &format!("jc/past-limit/{level}.jsonld"),
                &format!(r#"{{"@context": "{served}jc/past-limit/{}.jsonld"}}"#, level + 1),
                Some("application/ld+json"),
            )
            .await?;
    }
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_type_expanded = format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?;
    let refused_id = namespace.urn("Device", "refused")?;
    let body_id = namespace.urn("Device", "body")?;
    let terms_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let deep_linked = tenant.with_context(Context::Single(contexts.reference("jc/past-limit/1.jsonld")?))?;
    let deep_body = tenant
        .with_context(Context::Multiple(vec![contexts.reference("jc/past-limit/1.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let deep_query_body = tenant
        .with_context(Context::Single(contexts.reference("jc/past-limit/1.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 3)
        .build()?;
    terms_linked.entities().create(&device).send().await?.done()?;
    let still_serving = async || -> Result<()> {
        let retrieved = terms_linked
            .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
            .entities()
            .retrieve(&device_id)
            .send()
            .await?;
        verify_that!(retrieved.body().id, eq(&device_id))
    };
    let answer = deep_linked
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    still_serving().await?;
    let refused = Entity::builder().id(refused_id.clone()).entity_type(device_type.clone()).build()?;
    let answer_2 = deep_linked.with_responses().entities().create(&refused).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    still_serving().await?;
    let fragment = EntityFragment::builder().property("speed", 2).build()?;
    let answer_3 = deep_linked.with_responses().entities().update_attributes(&device_id, &fragment).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    still_serving().await?;
    let by_body = Entity::builder().id(body_id.clone()).entity_type(device_type.clone()).build()?;
    let answer_4 = deep_body.with_responses().entities().create(&by_body).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_5 = deep_query_body
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&device_type_expanded)
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    still_serving().await?;
    let plain = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(plain.body(), has_attribute(&namespace.expanded_term("speed")?, any_attribute().value(3)));
    for absent_id in [&refused_id, &body_id] {
        let absent = tenant.with_responses().with_accept(Accept::any()).entities().retrieve(absent_id).send().await;
        expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    }
    let listing = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(listed_wrapper_ids(&listing)?, is_empty());
    expect_that!(contexts.downloads("jc/past-limit/9.jsonld").await?, eq(0));
    Ok(())
}
