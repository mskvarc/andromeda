//! Post an entity selector with an id pattern answers the matching ids.

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
    query::IdPattern,
};

/// Pins: Query Entities by POST with an `EntitySelector` holding `idPattern` and type answers exactly the Entities
/// of that type whose id matches the pattern
/// Clause: 5.2.23, 5.2.33, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_0012_post_an_entity_selector_with_an_id_pattern_answers_the_matching_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_pat_1_id = namespace.urn("Building", "pat-1")?;
    let building_type = namespace.entity_type("Building")?;
    let building_pat_2_id = namespace.urn("Building", "pat-2")?;
    let building_other_1_id = namespace.urn("Building", "other-1")?;
    let entities = [
        Entity::builder().id(building_pat_1_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_pat_2_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_other_1_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-pat-.*").parse::<IdPattern>()?)
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_pat_1_id)));
    expect_that!(page.items(), contains(has_id(&building_pat_2_id)));
    expect_that!(page.items(), not(contains(has_id(&building_other_1_id))));
    // the type as a URI with no user context
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-pat-.*").parse::<IdPattern>()?)
        .entity_type(format!("https://uri.etsi.org/ngsi-ld/default-context/Building_{namespace}").as_str())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items().first().map(member_count), some(eq(2)));
    expect_that!(page_2.items().get(1).map(member_count), some(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_pat_1_id)));
    expect_that!(page_2.items(), contains(has_id(&building_pat_2_id)));
    expect_that!(page_2.items(), not(contains(has_id(&building_other_1_id))));
    expect_that!(page_2.items(), nth(0, has_type(&building_type)));
    expect_that!(page_2.items(), nth(1, has_type(&building_type)));
    Ok(())
}
