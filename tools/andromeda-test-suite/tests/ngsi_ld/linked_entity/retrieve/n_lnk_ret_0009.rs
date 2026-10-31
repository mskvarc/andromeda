//! Contained by keeps a held target out of a flat join.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join=flat does not append a target `containedBy` lists as previously encountered: the
/// array holds the retrieved entity alone, its Relationship unchanged, where the same join without `containedBy`
/// appends the target
/// Clause: 4.5.23, 5.7.1.4, 5.7.1.5, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0009_contained_by_keeps_a_held_target_out_of_a_flat_join() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_target_id = namespace.urn("Building", "target")?;
    let building_type = namespace.entity_type("Building")?;
    let building_main_id = namespace.urn("Building", "main")?;
    let entities = [
        Entity::builder().id(building_target_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(building_main_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_target_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_main_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_2 = retrieved.body();
    expect_that!(entities_2.as_slice(), len(eq(2)));
    expect_that!(entities_2.as_slice(), nth(1, has_id(&building_target_id)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_main_id)
        .join_level(1.try_into()?)
        .contained_by(vec![building_target_id.clone()])
        .send()
        .await?;
    let entities_3 = retrieved_2.body();
    expect_that!(entities_3.as_slice(), len(eq(1)));
    expect_that!(
        entities_3.as_slice(),
        nth(
            0,
            all!(
                has_id(&building_main_id),
                has_attribute("linkedTo", any_attribute().object(building_target_id.clone()))
            )
        )
    );
    Ok(())
}
