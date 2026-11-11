//! Without a link lists expanded iris.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Context, ContextEntry, Entity, InlineContext, Property},
};

/// Pins: Retrieve Available Attributes without a Link header lists the attributes of existing entities by their
/// expanded IRIs, which the core @context alone does not compact
/// Clause: 5.7.8.4, 5.2.27, 6.27.3.1, 6.3.5
#[gtest]
#[tokio::test]
async fn n_dsc_att_0002_without_a_link_lists_expanded_iris() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name_{namespace}": "https://test.andromeda/{namespace}/name", "subCategory_{namespace}": "https://test.andromeda/{namespace}/subCategory", "airQualityLevel_{namespace}": "https://test.andromeda/{namespace}/airQualityLevel", "almostFull_{namespace}": "https://test.andromeda/{namespace}/almostFull"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type)
        .property(namespace.attribute("name")?, "Eiffel Tower")
        .property(namespace.attribute("subCategory")?, "tourism")
        .property(
            namespace.attribute("airQualityLevel")?,
            Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?),
        )
        .property(namespace.attribute("almostFull")?, false)
        .build()?;
    linked.entities().create(&building).send().await?.done()?;
    let attributes = broker.with_accept(Accept::any()).attributes().list().send().await?;
    expect_that!(
        attributes.attribute_list,
        contains(eq(&format!("https://test.andromeda/{namespace}/name").parse::<AttributeName>()?))
    );
    expect_that!(
        attributes.attribute_list,
        contains(eq(&format!("https://test.andromeda/{namespace}/subCategory").parse::<AttributeName>()?))
    );
    expect_that!(
        attributes.attribute_list,
        contains(eq(&format!("https://test.andromeda/{namespace}/airQualityLevel").parse::<AttributeName>()?))
    );
    expect_that!(
        attributes.attribute_list,
        contains(eq(&format!("https://test.andromeda/{namespace}/almostFull").parse::<AttributeName>()?))
    );
    expect_that!(
        attributes.attribute_list,
        not(contains(eq(&format!("name_{namespace}").parse::<AttributeName>()?)))
    );
    expect_that!(
        attributes.attribute_list,
        not(contains(eq(&format!("subCategory_{namespace}").parse::<AttributeName>()?)))
    );
    expect_that!(
        attributes.attribute_list,
        not(contains(eq(&format!("airQualityLevel_{namespace}").parse::<AttributeName>()?)))
    );
    expect_that!(
        attributes.attribute_list,
        not(contains(eq(&format!("almostFull_{namespace}").parse::<AttributeName>()?)))
    );
    Ok(())
}
