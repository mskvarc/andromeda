//! Post an entity selector with id and type answers that entity.

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
};

/// Pins: Query Entities by POST with an `EntitySelector` holding id and type answers exactly that Entity, under a
/// body @context or with the type as a URI
/// Clause: 5.2.23, 5.2.33, 5.7.2, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_0010_post_an_entity_selector_with_id_and_type_answers_that_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_3_id).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .ids(vec![building_id.clone()])
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items().first().map(member_count), some(eq(2)));
    expect_that!(page.items(), nth(0, all!(has_id(&building_id), has_type(&building_type))));
    // the type as a URI with no user context
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .ids(vec![building_id.clone()])
        .entity_type(format!("https://uri.etsi.org/ngsi-ld/default-context/Building_{namespace}").as_str())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items().first().map(member_count), some(eq(2)));
    expect_that!(page_2.items(), nth(0, all!(has_id(&building_id), has_type(&building_type))));
    Ok(())
}
