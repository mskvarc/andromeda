//! A version profile on the context link keeps the context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_context,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use iri_rs::IriBuf;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext, NgsiLdMediaType, OneOrMany},
};

/// Pins: a create whose context Link carries the version profile parameter is admitted and expands through the
/// linked @context, the core one or a user one alike, so the entity reads back under its id and type
/// Clause: 6.3.5, 6.3.21, 5.6.1
#[gtest]
#[tokio::test]
async fn n_cmn_hdr_0001_a_version_profile_on_the_context_link_keeps_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let profile = IriBuf::new("https://uri.etsi.org/ngsi-ld/profile/version".to_owned())?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_type = namespace.entity_type("Building")?;
    let building_full_type = format!("https://test.andromeda/{namespace}/Building").parse::<EntityTypeName>()?;
    let user_context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name"}}"#)
            .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &user_context).await?;
    let core = broker.with_context(core_context()?)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "test")
        .build()?;
    core.with_context_profile(profile.clone())?.entities().create(&building).send().await?.done()?;
    let retrieved = core
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_attribute("name", any_attribute().value("test")));
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let building_2 = Entity::builder()
        .id(building_2_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    linked.with_context_profile(profile)?.entities().create(&building_2).send().await?.done()?;
    let retrieved_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&building_2_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type)));
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Eiffel Tower")));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.id, eq(&building_2_id));
    expect_that!(entity_3.entity_type, eq(&OneOrMany::one(building_full_type)));
    expect_that!(
        entity_3,
        has_attribute(&format!("https://test.andromeda/{namespace}/name"), any_attribute().value("Eiffel Tower"))
    );
    expect_that!(entity_3, lacks_attribute("name"));
    Ok(())
}
