//! An absolute term iri is kept verbatim.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext, NgsiLdMediaType, OneOrMany},
};

/// Pins: Under an inline @context defining Place and address by absolute IRIs, Create Entity and Batch Entity
/// Creation both store those IRIs
/// Clause: 5.5.7, 5.6.1, 5.6.7, JSON-LD 1.1 IRI expansion
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9049_an_absolute_term_iri_is_kept_verbatim() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"Place_{namespace}": "http://example.org/{namespace}/Place", "address": {{"@id": "http://example.org/{namespace}/address"}}}}"#)
                .parse::<InlineContext>()?,
        )))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let place_absolute_single_id = namespace.urn("Place", "absolute-single")?;
    let place_type = namespace.entity_type("Place")?;
    let place_absolute_batched_id = namespace.urn("Place", "absolute-batched")?;
    let place_type_2 = format!("http://example.org/{namespace}/Place").parse::<EntityTypeName>()?;
    let place_absolute_single = Entity::builder()
        .id(place_absolute_single_id.clone())
        .entity_type(place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?;
    body_context.entities().create(&place_absolute_single).send().await?.done()?;
    let entities = [Entity::builder()
        .id(place_absolute_batched_id.clone())
        .entity_type(place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?];
    body_context.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_absolute_single_id)
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
        .retrieve(&place_absolute_batched_id)
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
