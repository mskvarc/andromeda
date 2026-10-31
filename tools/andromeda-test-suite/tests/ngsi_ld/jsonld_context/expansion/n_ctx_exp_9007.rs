//! A cached context expires with its expires header.

use andromeda_test_suite::{
    broker,
    entity_matcher::lacks_attribute,
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use chrono::{Duration, Utc};
use googletest::prelude::*;
use http::header::EXPIRES;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, NgsiLdMediaType, OneOrMany},
};

/// Pins: A remote @context served with an Expires header two seconds out and no Cache-Control is downloaded again
/// once that instant has passed
/// Clause: 6.3.16, RFC 7234 s5.3
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9007_a_cached_context_expires_with_its_expires_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let path = "ctx/expires.jsonld";
    let building_type = namespace.entity_type("Building")?;
    let document = |name: &str| {
        format!(
            r#"{{"@context": {{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/{name}"}}}}"#
        )
    };
    let expires = |seconds: i64| -> Result<String> {
        // Two seconds or one second past now: the addition cannot overflow.
        let instant = Utc::now() + Duration::seconds(seconds);
        Ok(instant.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
    };
    contexts
        .serve_answer(
            path,
            &document("name"),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(EXPIRES, expires(2)?.parse()?),
        )
        .await?;
    let building_id = namespace.urn("Building", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    broker
        .with_context(Context::Single(contexts.reference(path)?))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&building)
        .send()
        .await?
        .done()?;
    let first = contexts.downloads(path).await?;
    expect_that!(first, ge(1));
    // Once the entry has lapsed the next use downloads a document mapping name elsewhere, so the stored name is no
    // longer compacted.
    contexts
        .serve_answer(
            path,
            &document("renamed"),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(EXPIRES, expires(2)?.parse()?),
        )
        .await?;
    let linked = broker.with_context(Context::Single(contexts.reference(path)?))?;
    retried(async || {
        let retrieved = linked
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&building_id)
            .send()
            .await?;
        let entity = retrieved.body();
        verify_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())))?;
        verify_that!(entity, lacks_attribute("name"))?;
        Ok(())
    })
    .await?;
    expect_that!(contexts.downloads(path).await?, gt(first));
    Ok(())
}
