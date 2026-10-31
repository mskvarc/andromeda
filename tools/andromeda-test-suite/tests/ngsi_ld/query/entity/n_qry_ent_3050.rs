//! Attrs naming a full iri selects the entity under a user context in every get spelling.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
};

/// Pins: Query Entities by GET with attrs as the only selector selects and projects by an attribute named with its
/// full IRI, with a compact IRI of the user context, and with a mix of IRIs and short names, under a Link header
/// carrying a user context, with and without a type beside it
/// Clause: 5.5.7, 5.7.2.4, 6.4.3.2
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_qry_ent_3050_attrs_naming_a_full_iri_selects_the_entity_under_a_user_context_in_every_get_spelling() -> Result<()> {
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
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("airQualityLevel", 7)
            .build()?,
    ];
    linked.entities().batch_create(&entities).send().await?.done()?;
    // 5.5.7: an absolute IRI expands to itself, so attrs alone selects the entities carrying that attribute
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
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
    let page_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .attrs(vec!["subCategory".parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_id)));
    // a compact IRI of the user context
    let page_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .attrs(vec!["ex:subCategory".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(
        page_3.items(),
        nth(0, all!(has_id(&building_id), has_attribute("subCategory", any_attribute().value("tourism"))))
    );
    // an IRI beside a short name: the entities carrying either attribute are selected, each projected to the listed ones
    let page_4 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .attrs(vec![
            format!("https://test.andromeda/{namespace}/subCategory").parse()?,
            "airQualityLevel".parse()?,
        ])
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(2)));
    expect_that!(
        page_4.items(),
        contains(all!(
            has_id(&building_id),
            has_attribute("subCategory", any_attribute().value("tourism")),
            has_attribute("airQualityLevel", any_attribute().value(4))
        ))
    );
    expect_that!(
        page_4.items(),
        contains(all!(has_id(&building_2_id), has_attribute("airQualityLevel", any_attribute().value(7))))
    );
    // two IRIs
    let page_5 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .attrs(vec![
            format!("https://test.andromeda/{namespace}/subCategory").parse()?,
            format!("https://test.andromeda/{namespace}/airQualityLevel").parse()?,
        ])
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(2)));
    // beside a type selector
    let page_6 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .attrs(vec![format!("https://test.andromeda/{namespace}/subCategory").parse()?])
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&building_id)));
    // the representation options do not change the selection
    let page_7 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .attrs(vec![format!("https://test.andromeda/{namespace}/subCategory").parse()?])
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), nth(0, has_value("subCategory", "tourism")));
    Ok(())
}
