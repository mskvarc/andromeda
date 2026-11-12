//! A query repeated after a reload expands its terms by the new download.

use andromeda_test_suite::{
    broker,
    context_record,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::ContextDeletion},
    model::{Context, Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by type and a `q` on a term of a Cached @context, repeated unchanged after Delete @context
/// with reload=true replaced that @context with a download mapping the term to another IRI, answers only the entity
/// written under the new mapping
/// Clause: 5.13.5.4, 5.5.7, 4.9
#[gtest]
#[tokio::test]
async fn n_ctx_del_9007_a_query_repeated_after_a_reload_expands_its_terms_by_the_new_download() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    // The second download maps `speed` to another IRI.
    contexts
        .serve_then(
            "ctx/query.jsonld",
            (
                &format!(
                    r#"{{"@context": {{"Car_{namespace}": "https://test.andromeda/{namespace}/Car", "speed": "https://test.andromeda/{namespace}/speed"}}}}"#
                ),
                ServedAnswer::ok().with_media_type("application/ld+json"),
            ),
            (
                &format!(
                    r#"{{"@context": {{"Car_{namespace}": "https://test.andromeda/{namespace}/Car", "speed": "https://test.andromeda/{namespace}/velocity"}}}}"#
                ),
                ServedAnswer::ok().with_media_type("application/ld+json"),
            ),
        )
        .await?;
    let car_type = namespace.entity_type("Car")?;
    let in_body = broker
        .with_context(Context::Multiple(vec![contexts.reference("ctx/query.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let linked = broker
        .with_context(Context::Single(contexts.reference("ctx/query.jsonld")?))?
        .with_accept(Accept::new(NgsiLdMediaType::Json));
    let first_id = namespace.urn("Car", 1)?;
    let first = Entity::builder()
        .id(first_id.clone())
        .entity_type(car_type.clone())
        .property("speed", 50)
        .build()?;
    in_body.entities().create(&first).send().await?.done()?;
    let before = linked.entities().query().entity_type(&car_type).q(q::attr("speed").gt(10)).send().await?;
    expect_that!(before.items(), elements_are![has_id(&first_id)]);
    let url = contexts.url("ctx/query.jsonld")?;
    // A download is recorded after the request that made it, so its entry is read until it appears.
    context_record::cached_under(&broker, &url).await?;
    let record = context_record::stored(&broker, &context_record::segment_of(&url)).await?;
    broker.contexts().delete(&record.local_id).deletion(ContextDeletion::Reload).send().await?;
    let second_id = namespace.urn("Car", 2)?;
    let second = Entity::builder()
        .id(second_id.clone())
        .entity_type(car_type.clone())
        .property("speed", 50)
        .build()?;
    in_body.entities().create(&second).send().await?.done()?;
    let after = linked.entities().query().entity_type(&car_type).q(q::attr("speed").gt(10)).send().await?;
    expect_that!(after.items(), elements_are![has_id(&second_id)]);
    expect_that!(contexts.downloads("ctx/query.jsonld").await?, eq(2));
    Ok(())
}
