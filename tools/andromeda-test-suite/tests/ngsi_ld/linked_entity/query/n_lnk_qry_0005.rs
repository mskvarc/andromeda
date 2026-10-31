//! Sys attrs render system times on linked entities.

use andromeda_test_suite::{broker, identity_matcher::has_id, linked_entity::linked_entities, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Relationship, VocabTerm},
    query::Join,
};

/// Pins: Query Entities with `options=sysAttrs` and a join renders `createdAt` and `modifiedAt` on the Linked
/// Entities as on the matching entities, in inline and in flat form
/// Clause: 4.5.23, 4.8, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0005_sys_attrs_render_system_times_on_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let city_2_id = namespace.urn("City", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", Relationship::builder(city_id.clone()).object_type(VocabTerm::from(&city_type)))
            .build()?,
        Entity::builder().id(city_id.clone()).entity_type(city_type.clone()).build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", Relationship::builder(city_2_id.clone()).object_type(VocabTerm::from(&city_type)))
            .build()?,
        Entity::builder().id(city_2_id.clone()).entity_type(city_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Inline.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    let item = page.items().iter().find(|item| item.id == building_id).or_fail()?;
    expect_that!(item.output.created_at, some(anything()));
    expect_that!(item.output.modified_at, some(anything()));
    expect_that!(linked_entities(item, &["locatedAt"]), nth(0, has_id(&city_id)));
    expect_that!(
        linked_entities(item, &["locatedAt"])
            .first()
            .and_then(|linked| linked.output.created_at.as_ref()),
        some(anything())
    );
    expect_that!(
        linked_entities(item, &["locatedAt"])
            .first()
            .and_then(|linked| linked.output.modified_at.as_ref()),
        some(anything())
    );
    let item_2 = page.items().iter().find(|item| item.id == building_2_id).or_fail()?;
    expect_that!(item_2.output.created_at, some(anything()));
    expect_that!(item_2.output.modified_at, some(anything()));
    expect_that!(linked_entities(item_2, &["locatedAt"]), nth(0, has_id(&city_2_id)));
    expect_that!(
        linked_entities(item_2, &["locatedAt"])
            .first()
            .and_then(|linked| linked.output.created_at.as_ref()),
        some(anything())
    );
    expect_that!(
        linked_entities(item_2, &["locatedAt"])
            .first()
            .and_then(|linked| linked.output.modified_at.as_ref()),
        some(anything())
    );
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&building_2_id)));
    // Flat; every one of the four members carries the system timestamps.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(4)));
    let item_3 = page_2.items().iter().find(|item| item.id == building_id).or_fail()?;
    expect_that!(item_3.output.created_at, some(anything()));
    expect_that!(item_3.output.modified_at, some(anything()));
    let item_4 = page_2.items().iter().find(|item| item.id == building_2_id).or_fail()?;
    expect_that!(item_4.output.created_at, some(anything()));
    expect_that!(item_4.output.modified_at, some(anything()));
    let item_5 = page_2.items().iter().find(|item| item.id == city_id).or_fail()?;
    expect_that!(item_5.output.created_at, some(anything()));
    expect_that!(item_5.output.modified_at, some(anything()));
    let item_6 = page_2.items().iter().find(|item| item.id == city_2_id).or_fail()?;
    expect_that!(item_6.output.created_at, some(anything()));
    expect_that!(item_6.output.modified_at, some(anything()));
    expect_that!(page_2.items(), contains(has_id(&building_id)));
    expect_that!(page_2.items(), contains(has_id(&building_2_id)));
    expect_that!(page_2.items(), contains(has_id(&city_id)));
    expect_that!(page_2.items(), contains(has_id(&city_2_id)));
    Ok(())
}
