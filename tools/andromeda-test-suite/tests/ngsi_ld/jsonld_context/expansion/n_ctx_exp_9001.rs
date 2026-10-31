//! A batch create resolves prefixed terms like a single create.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, EntityTypeName, NgsiLdMediaType, OneOrMany},
};

/// Pins: Under a @context defining Place and address as compact IRIs over a prefix it defines, Create Entity and
/// Batch Entity Creation both answer 201, and both entities read back without a @context as the expanded type and
/// Attribute name
/// Clause: 5.5.7, 5.6.1.4, 5.6.7.4, JSON-LD 1.1 clause 4.1.2
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9001_a_batch_create_resolves_prefixed_terms_like_a_single_create() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/prefixed.jsonld")?))?;
    let place_single_id = namespace.urn("Place", "single")?;
    let place_type = namespace.entity_type("Place")?;
    let place_batched_id = namespace.urn("Place", "batched")?;
    let place_type_2 = format!("http://example.org/{namespace}/Place").parse::<EntityTypeName>()?;
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
    let place_single = Entity::builder()
        .id(place_single_id.clone())
        .entity_type(place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?;
    linked.entities().create(&place_single).send().await?.done()?;
    let entities = [Entity::builder()
        .id(place_batched_id.clone())
        .entity_type(place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?];
    linked.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_single_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(place_type_2.clone())));
    expect_that!(
        entity,
        has_attribute(
            &format!("http://example.org/{namespace}/address"),
            any_attribute().value("Pariser Platz, Berlin, Germany")
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_batched_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(place_type_2.clone())));
    expect_that!(
        entity_2,
        has_attribute(
            &format!("http://example.org/{namespace}/address"),
            any_attribute().value("Pariser Platz, Berlin, Germany")
        )
    );
    Ok(())
}
