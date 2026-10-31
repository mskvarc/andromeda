//! A batch created entity is reachable under the context that wrote it.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType, OneOrMany},
};

/// Pins: An entity created by Batch Entity Creation under a @context of prefixed compact IRIs is answered by
/// Retrieve Entity and by Query Entities type=Place under that same @context
/// Clause: 5.5.7, 5.6.7.4, 5.7.1.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9002_a_batch_created_entity_is_reachable_under_the_context_that_wrote_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/prefixed.jsonld")?))?;
    let place_id = namespace.urn("Place", 1)?;
    let place_type = namespace.entity_type("Place")?;
    contexts
        .serve(
            "ctx/prefixed.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "example": "http://example.org/{namespace}/",
        "Place_{namespace}": "example:Place",
        "address": "example:address"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let entities = [Entity::builder()
        .id(place_id.clone())
        .entity_type(place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?];
    linked.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(place_type.clone())));
    expect_that!(entity, has_attribute("address", any_attribute().value("Pariser Platz, Berlin, Germany")));
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&place_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&place_id)));
    Ok(())
}
