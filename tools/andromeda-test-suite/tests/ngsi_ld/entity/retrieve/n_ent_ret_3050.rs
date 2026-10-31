//! Attrs naming a full iri projects the entity under a user context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with attrs naming an attribute by its full IRI, by a compact IRI or by its short name
/// answers the entity reduced to that attribute under a Link header carrying a user context, and an IRI the entity
/// does not hold is `ResourceNotFound`
/// Clause: 5.5.7, 5.7.1.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_3050_attrs_naming_a_full_iri_projects_the_entity_under_a_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"ex": "https://test.andromeda/{namespace}/", "Building_{namespace}": "https://test.andromeda/{namespace}/Building", "subCategory": "https://test.andromeda/{namespace}/subCategory", "airQualityLevel": "https://test.andromeda/{namespace}/airQualityLevel", "floors": "https://test.andromeda/{namespace}/floors"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("subCategory", "tourism")
        .property("airQualityLevel", 4)
        .build()?;
    linked.entities().create(&building).send().await?.done()?;
    // 5.5.7: an absolute IRI expands to itself
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .attrs(vec![format!("https://test.andromeda/{namespace}/subCategory").parse()?])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, lacks_attribute("airQualityLevel"));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    let retrieved_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .attrs(vec!["ex:airQualityLevel".parse()?])
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&building_id));
    expect_that!(entity_2, lacks_attribute("subCategory"));
    expect_that!(entity_2, has_attribute("airQualityLevel", any_attribute().value(4)));
    let retrieved_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .attrs(vec!["subCategory".parse()?])
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.id, eq(&building_id));
    expect_that!(entity_3, lacks_attribute("airQualityLevel"));
    expect_that!(entity_3, has_attribute("subCategory", any_attribute().value("tourism")));
    // an IRI of an attribute the entity does not hold selects nothing of it
    let answer = linked
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .attrs(vec![format!("https://test.andromeda/{namespace}/floors").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
