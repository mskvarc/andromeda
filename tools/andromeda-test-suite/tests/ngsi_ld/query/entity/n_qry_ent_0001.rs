//! An id list selects exactly the listed entities.

use andromeda_test_suite::{broker, entity_matcher::member_count, identity_matcher::has_id, namespace::Namespace, outcome::Done, type_matcher::has_type};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: an id list selects exactly the listed entities of the type, here given as its full IRI, leaving the third
/// one out
/// Clause: 5.7.2.3, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ent_0001_an_id_list_selects_exactly_the_listed_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
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
    // The type named by its full IRI (clause 5.7.2.3 admits both spellings)
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_id.clone(), building_2_id.clone()])
        .entity_type(format!("https://uri.etsi.org/ngsi-ld/default-context/Building_{namespace}").as_str())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items().first().map(member_count), some(eq(2)));
    expect_that!(page.items().get(1).map(member_count), some(eq(2)));
    expect_that!(page.items(), contains(all!(has_id(&building_id), has_type(&building_type))));
    expect_that!(page.items(), contains(all!(has_id(&building_2_id), has_type(&building_type))));
    expect_that!(page.items(), not(contains(has_id(&building_3_id))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_id.clone(), building_2_id.clone()])
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(all!(has_id(&building_id), has_type(&building_type))));
    expect_that!(page_2.items(), contains(all!(has_id(&building_2_id), has_type(&building_type))));
    Ok(())
}
