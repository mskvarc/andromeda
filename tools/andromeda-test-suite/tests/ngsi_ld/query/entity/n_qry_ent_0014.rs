//! Post entity selectors are disjunctive.

use andromeda_test_suite::{
    broker,
    entity_matcher::member_count,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities by POST with two `EntitySelectors` answers the Entities either of them selects and no other
/// Clause: 5.2.23, 5.2.33, 6.23.3.1
/// Tenant: private (the broker forwards a query of several selectors to registrations of other types too, so in the
/// shared tenant it reaches other files' sources)
#[gtest]
#[tokio::test]
async fn n_qry_ent_0014_post_entity_selectors_are_disjunctive() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_3_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let selectors = [
        EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_id.clone()),
        EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_2_id.clone()),
    ];
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&building_2_id)));
    expect_that!(page.items(), not(contains(has_id(&building_3_id))));
    // the types as URIs with no user context
    let selectors_2 = [
        EntitySelector::new(SelectorTypeBuilder::from(format!("https://uri.etsi.org/ngsi-ld/default-context/Building_{namespace}").as_str()).build()?)
            .with_ids(building_id.clone()),
        EntitySelector::new(SelectorTypeBuilder::from(format!("https://uri.etsi.org/ngsi-ld/default-context/Building_{namespace}").as_str()).build()?)
            .with_ids(building_2_id.clone()),
    ];
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors_2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items().first().map(member_count), some(eq(2)));
    expect_that!(page_2.items().get(1).map(member_count), some(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_id)));
    expect_that!(page_2.items(), contains(has_id(&building_2_id)));
    expect_that!(page_2.items(), not(contains(has_id(&building_3_id))));
    expect_that!(page_2.items(), nth(0, has_type(&building_type)));
    expect_that!(page_2.items(), nth(1, has_type(&building_type)));
    Ok(())
}
