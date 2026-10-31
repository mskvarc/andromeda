//! A compact iri term definition resolves alike on every write path.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, EntityFragment, EntityTypeName, InlineContext, NgsiLdMediaType, OneOrMany},
};

/// Pins: Under an inline @context defining Place and address as compact IRIs over a prefix it defines, Create
/// Entity, Batch Entity Creation, Upsert, Update and Merge each store the expanded type and Attribute name and
/// never the compact IRI, and Query Entities by the expanded type answers all five
/// Clause: 5.5.7, 5.6.1, 5.6.7, 5.6.8, 5.6.9, 5.6.20, JSON-LD 1.1 IRI expansion
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9047_a_compact_iri_term_definition_resolves_alike_on_every_write_path() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"example": "http://example.org/{namespace}/", "Place_{namespace}": "example:Place", "address": "example:address"}}"#)
                .parse::<InlineContext>()?,
        )))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let place_prefix_single_id = namespace.urn("Place", "prefix-single")?;
    let place_type = namespace.entity_type("Place")?;
    let place_prefix_created_id = namespace.urn("Place", "prefix-created")?;
    let place_prefix_upserted_id = namespace.urn("Place", "prefix-upserted")?;
    let place_prefix_updated_id = namespace.urn("Place", "prefix-updated")?;
    let place_prefix_merged_id = namespace.urn("Place", "prefix-merged")?;
    let place_type_2 = format!("http://example.org/{namespace}/Place").parse::<EntityTypeName>()?;
    let place_prefix_single = Entity::builder()
        .id(place_prefix_single_id.clone())
        .entity_type(place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?;
    body_context.entities().create(&place_prefix_single).send().await?.done()?;
    let entities = [Entity::builder()
        .id(place_prefix_created_id.clone())
        .entity_type(place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?];
    body_context.entities().batch_create(&entities).send().await?.done()?;
    let entities_2 = [Entity::builder()
        .id(place_prefix_upserted_id.clone())
        .entity_type(place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?];
    body_context.entities().batch_upsert(&entities_2).send().await?.done()?;
    let place_prefix_updated = Entity::builder().id(place_prefix_updated_id.clone()).entity_type(place_type.clone()).build()?;
    body_context.entities().create(&place_prefix_updated).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(place_prefix_updated_id.clone(), place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?];
    body_context.entities().batch_update(&fragments).send().await?.done()?;
    let place_prefix_merged = Entity::builder().id(place_prefix_merged_id.clone()).entity_type(place_type.clone()).build()?;
    body_context.entities().create(&place_prefix_merged).send().await?.done()?;
    let fragments_2 = [EntityFragment::for_entity(place_prefix_merged_id.clone(), place_type.clone())
        .property("address", "Pariser Platz, Berlin, Germany")
        .build()?];
    body_context.entities().batch_merge(&fragments_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_prefix_single_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(place_type_2.clone())));
    expect_that!(entity, lacks_attribute("example:address"));
    expect_that!(entity, has_attribute(&format!("http://example.org/{namespace}/address"), any_attribute()));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_prefix_created_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(place_type_2.clone())));
    expect_that!(entity_2, lacks_attribute("example:address"));
    expect_that!(entity_2, has_attribute(&format!("http://example.org/{namespace}/address"), any_attribute()));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_prefix_upserted_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.entity_type, eq(&OneOrMany::one(place_type_2.clone())));
    expect_that!(entity_3, lacks_attribute("example:address"));
    expect_that!(entity_3, has_attribute(&format!("http://example.org/{namespace}/address"), any_attribute()));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_prefix_updated_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4.entity_type, eq(&OneOrMany::one(place_type_2.clone())));
    expect_that!(entity_4, lacks_attribute("example:address"));
    expect_that!(entity_4, has_attribute(&format!("http://example.org/{namespace}/address"), any_attribute()));
    let retrieved_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&place_prefix_merged_id)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(entity_5.entity_type, eq(&OneOrMany::one(place_type_2.clone())));
    expect_that!(entity_5, lacks_attribute("example:address"));
    expect_that!(entity_5, has_attribute(&format!("http://example.org/{namespace}/address"), any_attribute()));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&place_type_2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(5)));
    expect_that!(page.items(), contains(has_id(&place_prefix_single_id)));
    expect_that!(page.items(), contains(has_id(&place_prefix_created_id)));
    expect_that!(page.items(), contains(has_id(&place_prefix_upserted_id)));
    expect_that!(page.items(), contains(has_id(&place_prefix_updated_id)));
    expect_that!(page.items(), contains(has_id(&place_prefix_merged_id)));
    Ok(())
}
