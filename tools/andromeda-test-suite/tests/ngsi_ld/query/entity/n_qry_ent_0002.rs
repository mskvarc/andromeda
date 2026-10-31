//! An id pattern selects exactly the matching ids.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, type_matcher::has_type};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: `idPattern` selects exactly the entities of the type whose id matches the regular expression
/// Clause: 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ent_0002_an_id_pattern_selects_exactly_the_matching_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_idpat_a_id = namespace.urn("Building", "idpat-a")?;
    let building_type = namespace.entity_type("Building")?;
    let building_idpat_b_id = namespace.urn("Building", "idpat-b")?;
    let building_other_id = namespace.urn("Building", "other")?;
    let entities = [
        Entity::builder().id(building_idpat_a_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_idpat_b_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_other_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .id_pattern(namespace.id_pattern_ending("Building", "idpat.*")?)
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(all!(has_id(&building_idpat_a_id), has_type(&building_type))));
    expect_that!(page.items(), contains(all!(has_id(&building_idpat_b_id), has_type(&building_type))));
    expect_that!(page.items(), not(contains(has_id(&building_other_id))));
    // A pattern matching every id of the file selects all three entities
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .id_pattern(namespace.id_pattern("Building")?)
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    Ok(())
}
