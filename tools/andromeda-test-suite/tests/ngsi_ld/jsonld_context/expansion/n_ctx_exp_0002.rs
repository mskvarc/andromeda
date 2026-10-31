//! A json create without a link reads under the default context.

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

/// Pins: an application/json Create Entity without a Link header expands its terms with the default @context, so the
/// entity reads back under its own names without a context, and under the default-context prefix when a context
/// naming those terms otherwise is linked
/// Clause: 6.3.5, 5.5.7, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0002_a_json_create_without_a_link_uses_the_default_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let name = format!("name_{namespace}");
    let sub_category = format!("subCategory_{namespace}");
    let almost_full = format!("almostFull_{namespace}");
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property(name.as_str(), "Eiffel Tower")
        .property(sub_category.as_str(), "tourism")
        .property(almost_full.as_str(), false)
        .build()?;
    let received = broker.with_responses().entities().create(&building).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(contains_substring(building_id.as_str()))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_attribute(&name, any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute(&sub_category, any_attribute().value("tourism")));
    expect_that!(entity, has_attribute(&almost_full, any_attribute().value(false)));
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "{name}": "https://test.andromeda/{namespace}/name", "{sub_category}": "https://test.andromeda/{namespace}/subCategory", "{almost_full}": "https://test.andromeda/{namespace}/almostFull"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let default_type: EntityTypeName = format!("ngsi-ld:default-context/{building_type}").parse()?;
    let retrieved_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&building_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(default_type)));
    expect_that!(entity_2, lacks_attribute(&name));
    expect_that!(entity_2, lacks_attribute(&almost_full));
    expect_that!(
        entity_2,
        has_attribute(&format!("ngsi-ld:default-context/{name}"), any_attribute().value("Eiffel Tower"))
    );
    expect_that!(
        entity_2,
        has_attribute(&format!("ngsi-ld:default-context/{sub_category}"), any_attribute().value("tourism"))
    );
    expect_that!(
        entity_2,
        has_attribute(&format!("ngsi-ld:default-context/{almost_full}"), any_attribute().value(false))
    );
    Ok(())
}
