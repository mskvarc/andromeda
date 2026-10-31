//! Post query attrs naming a full iri selects the entity under a user context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
};

/// Pins: Query Entities by POST with an attrs member as the only selector selects and projects by an attribute
/// named with its full IRI, with a compact IRI and with a short name, under a body @context and under a Link header
/// carrying a user context
/// Clause: 5.5.7, 5.7.2.4, 5.2.23, 6.23.3.1
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_qry_ent_3051_post_query_attrs_naming_a_full_iri_selects_the_entity_under_a_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"ex": "https://test.andromeda/{namespace}/", "Building_{namespace}": "https://test.andromeda/{namespace}/Building", "subCategory": "https://test.andromeda/{namespace}/subCategory", "airQualityLevel": "https://test.andromeda/{namespace}/airQualityLevel"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("subCategory", "tourism")
            .property("airQualityLevel", 4)
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("airQualityLevel", 7)
            .build()?,
    ];
    linked.entities().batch_create(&entities).send().await?.done()?;
    // 5.5.7: an absolute IRI expands to itself; the body context is the user context
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let page = body_context
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .attrs(vec![format!("https://test.andromeda/{namespace}/subCategory").parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_attribute("subCategory", any_attribute().value("tourism")),
                lacks_attribute("airQualityLevel")
            )
        )
    );
    // the short name, the control
    let page_2 = body_context
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .attrs(vec!["subCategory".parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_id)));
    // a compact IRI
    let page_3 = body_context
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .attrs(vec!["ex:subCategory".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_id)));
    // the user context given by Link with an application/json body
    let page_4 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .attrs(vec![format!("https://test.andromeda/{namespace}/subCategory").parse()?])
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(
        page_4.items(),
        nth(0, all!(has_id(&building_id), has_attribute("subCategory", any_attribute().value("tourism"))))
    );
    // several names, an IRI among them
    let page_5 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .attrs(vec![
            format!("https://test.andromeda/{namespace}/subCategory").parse()?,
            "airQualityLevel".parse()?,
        ])
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(2)));
    Ok(())
}
