//! Without a link lists expanded iris.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext},
};

/// Pins: Retrieve Available Entity Types without a Link header lists the types of existing entities by their
/// expanded IRIs, which the core @context alone does not compact
/// Clause: 5.7.5.4, 5.2.24, 6.25.3.1, 6.3.5
#[gtest]
#[tokio::test]
async fn n_dsc_typ_0002_without_a_link_lists_expanded_iris() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type_2 = format!("https://test.andromeda/{namespace}/Building").parse::<EntityTypeName>()?;
    let vehicle_type_2 = format!("https://test.andromeda/{namespace}/Vehicle").parse::<EntityTypeName>()?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "name_{namespace}": "https://test.andromeda/{namespace}/name", "brandName_{namespace}": "https://test.andromeda/{namespace}/brandName"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property(namespace.attribute("name")?, "Eiffel Tower")
            .build()?,
        Entity::builder()
            .id(vehicle_id)
            .entity_type(vehicle_type.clone())
            .property(namespace.attribute("brandName")?, "Mercedes")
            .build()?,
    ];
    linked.entities().batch_create(&entities).send().await?.done()?;
    let types = broker.with_accept(Accept::any()).entity_types().list().send().await?;
    expect_that!(types.type_list, contains(eq(&building_type_2)));
    expect_that!(types.type_list, contains(eq(&vehicle_type_2)));
    expect_that!(types.type_list, not(contains(eq(&building_type))));
    expect_that!(types.type_list, not(contains(eq(&vehicle_type))));
    Ok(())
}
