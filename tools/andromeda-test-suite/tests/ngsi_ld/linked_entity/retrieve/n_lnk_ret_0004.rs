//! Join with sys attrs stamps the linked entities.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{InlineJoin, SystemAttributes},
    },
    model::{Entity, NgsiLdMediaType, Relationship, VocabTerm},
};

/// Pins: Retrieve Entity with `options=sysAttrs` and a join carries `createdAt` and `modifiedAt` on the Linked
/// Entities too, inline and flat, as JSON and as JSON-LD
/// Clause: 5.7.1.4, 4.5.23, 4.8
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0004_join_with_sys_attrs_stamps_the_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let city_paris_id = namespace.urn("City", "paris")?;
    let city_type = namespace.entity_type("City")?;
    let building_typed_id = namespace.urn("Building", "typed")?;
    let building_type = namespace.entity_type("Building")?;
    let building_untyped_id = namespace.urn("Building", "untyped")?;
    let entities = [
        Entity::builder().id(city_paris_id.clone()).entity_type(city_type.clone()).build()?,
        Entity::builder()
            .id(building_typed_id.clone())
            .entity_type(building_type.clone())
            .relationship(
                "locatedAt",
                Relationship::builder(city_paris_id.clone()).object_type(VocabTerm::from(&city_type)),
            )
            .build()?,
        Entity::builder()
            .id(building_untyped_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_typed_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(system_time::of_entity(entity, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(entity.id, eq(&building_typed_id));
    expect_that!(linked_entities(entity, &["locatedAt"]), nth(0, has_id(&city_paris_id)));
    expect_that!(
        linked_entities(entity, &["locatedAt"])
            .first()
            .and_then(|linked| linked.output.created_at.as_ref()),
        some(anything())
    );
    expect_that!(
        linked_entities(entity, &["locatedAt"])
            .first()
            .and_then(|linked| linked.output.modified_at.as_ref()),
        some(anything())
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_typed_id)
        .join_level(1.try_into()?)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entities_2 = retrieved_2.body();
    expect_that!(entities_2.as_slice(), len(eq(2)));
    let item = entities_2.first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(system_time::of_entity(item, SystemMember::ModifiedAt).ok(), some(anything()));
    let item_2 = entities_2.get(1).or_fail()?;
    expect_that!(system_time::of_entity(item_2, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(system_time::of_entity(item_2, SystemMember::ModifiedAt).ok(), some(anything()));
    expect_that!(entities_2.as_slice(), nth(0, has_id(&building_typed_id)));
    expect_that!(entities_2.as_slice(), nth(1, has_id(&city_paris_id)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_untyped_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_3.body();
    expect_that!(entity_2.id, eq(&building_untyped_id));
    expect_that!(linked_entities(entity_2, &["locatedAt"]), nth(0, has_id(&city_paris_id)));
    expect_that!(
        linked_entities(entity_2, &["locatedAt"])
            .first()
            .and_then(|linked| linked.output.created_at.as_ref()),
        some(anything())
    );
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_flattened(&building_untyped_id)
        .join_level(1.try_into()?)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entities_3 = retrieved_4.body();
    expect_that!(entities_3.as_slice(), len(eq(2)));
    let item_3 = entities_3.first().or_fail()?;
    expect_that!(system_time::of_entity(item_3, SystemMember::CreatedAt).ok(), some(anything()));
    let item_4 = entities_3.get(1).or_fail()?;
    expect_that!(system_time::of_entity(item_4, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(entities_3.as_slice(), nth(0, has_id(&building_untyped_id)));
    expect_that!(entities_3.as_slice(), nth(1, has_id(&city_paris_id)));
    Ok(())
}
