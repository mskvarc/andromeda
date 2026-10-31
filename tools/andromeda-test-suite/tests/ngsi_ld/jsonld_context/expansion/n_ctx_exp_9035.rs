//! An imported scoped context is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: Create Entity whose body @context imports a context defining gear with a scoped @context, or under a Link
/// naming such an importing context, answers 400 `BadRequestData`, while one importing a plain definition of gear
/// is used and expands gear to its IRI
/// Clause: 5.5.7, 6.3.5
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9035_an_imported_scoped_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "contexts/scoped.jsonld",
            &format!(
                r#"{{"@context": {{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}}}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "contexts/plain.jsonld",
            &format!(r#"{{"@context": {{"gear": "https://example.org/{namespace}/gear"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let scoped = contexts.url("contexts/scoped.jsonld")?;
    let plain = contexts.url("contexts/plain.jsonld")?;
    contexts
        .serve(
            "contexts/importing.jsonld",
            &format!(r#"{{"@context": {{"@import": "{scoped}", "axle": "https://example.org/{namespace}/axle"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let device_type = namespace.entity_type("Device")?;
    let inline_id = namespace.urn("Device", "import-inline")?;
    let linked_id = namespace.urn("Device", "import-linked")?;
    let plain_id = namespace.urn("Device", "import-plain")?;
    let importing_scoped = broker
        .with_context(Context::Multiple(vec![
            core_entry()?,
            ContextEntry::Inline(format!(r#"{{"@import": "{scoped}", "axle": "https://example.org/{namespace}/axle"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let inline_device = Entity::builder()
        .id(inline_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 3)
        .build()?;
    let answer = importing_scoped.with_responses().entities().create(&inline_device).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/importing.jsonld")?))?;
    let linked_device = Entity::builder()
        .id(linked_id.clone())
        .entity_type(device_type.clone())
        .property("axle", 2)
        .build()?;
    let answer_2 = linked.with_responses().entities().create(&linked_device).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let importing_plain = broker
        .with_context(Context::Multiple(vec![
            core_entry()?,
            ContextEntry::Inline(format!(r#"{{"@import": "{plain}", "axle": "https://example.org/{namespace}/axle"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let plain_device = Entity::builder().id(plain_id.clone()).entity_type(device_type).property("gear", 3).build()?;
    importing_plain.entities().create(&plain_device).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&plain_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("https://example.org/{namespace}/gear"), any_attribute().value(3))
    );
    for absent_id in [&inline_id, &linked_id] {
        let absent = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(absent_id)
            .send()
            .await;
        expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    }
    Ok(())
}
