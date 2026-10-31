//! A json create reads its terms through the link context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext, NgsiLdMediaType, OneOrMany},
};

/// Pins: an application/json Create Entity expands its terms with the @context its Link header names, so the entity
/// reads back under those terms with that context and under the full IRIs without it
/// Clause: 6.3.5, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0001_a_json_create_reads_its_terms_through_the_link_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_type_2 = format!("https://test.andromeda/{namespace}/Building").parse::<EntityTypeName>()?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name", "subCategory": "https://test.andromeda/{namespace}/subCategory", "almostFull": "https://test.andromeda/{namespace}/almostFull"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .property("almostFull", false)
        .build()?;
    let received = linked.with_responses().entities().create(&building).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(contains_substring(format!("urn:ngsi-ld:Building:{namespace}-1")))
    );
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    expect_that!(entity, has_attribute("almostFull", any_attribute().value(false)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&building_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type_2)));
    expect_that!(entity_2, lacks_attribute("name"));
    expect_that!(entity_2, lacks_attribute("subCategory"));
    expect_that!(entity_2, lacks_attribute("almostFull"));
    expect_that!(
        entity_2,
        has_attribute(&format!("https://test.andromeda/{namespace}/name"), any_attribute().value("Eiffel Tower"))
    );
    expect_that!(
        entity_2,
        has_attribute(&format!("https://test.andromeda/{namespace}/subCategory"), any_attribute().value("tourism"))
    );
    expect_that!(
        entity_2,
        has_attribute(&format!("https://test.andromeda/{namespace}/almostFull"), any_attribute().value(false))
    );
    Ok(())
}
