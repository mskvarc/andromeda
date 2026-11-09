//! Both join modes narrow a linked entity identity by the nested terms.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick=id,linkedTo{id,name} answers the Linked Entity with id and name and no type,
/// nested by join=inline and appended by join=flat, and with pick=id,linkedTo{name} under join=flat answers it with
/// name and neither id nor type
/// Clause: 4.21, 4.5.23, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9007_both_join_modes_narrow_a_linked_entity_identity_by_the_nested_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_linked_identity_id = namespace.urn("Building", "linked-identity")?;
    let building_type = namespace.entity_type("Building")?;
    let city_linked_identity_id = namespace.urn("City", "linked-identity")?;
    let city_type = namespace.entity_type("City")?;
    let country_linked_identity_id = namespace.urn("Country", "linked-identity")?;
    let country_type = namespace.entity_type("Country")?;
    let building_linked_identity = Entity::builder()
        .id(building_linked_identity_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("linkedTo", city_linked_identity_id.clone())
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building_linked_identity).send().await?.done()?;
    let city_linked_identity = Entity::builder()
        .id(city_linked_identity_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .relationship("linkedTo", country_linked_identity_id.clone())
        .build()?;
    broker.entities().create(&city_linked_identity).send().await?.done()?;
    let country_linked_identity = Entity::builder()
        .id(country_linked_identity_id.clone())
        .entity_type(country_type)
        .property("name", "France")
        .build()?;
    broker.entities().create(&country_linked_identity).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_linked_identity_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .pick("id,linkedTo{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(linked_entities(entity, &["linkedTo"]), nth(0, has_id(&city_linked_identity_id)));
    expect_that!(linked_entities(entity, &["linkedTo"]), nth(0, has_attribute("name", any_attribute())));
    expect_that!(
        linked_entities(entity, &["linkedTo"]).first().and_then(|linked| linked.entity_type.as_ref()),
        none()
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_linked_identity_id)
        .join_level(1.try_into()?)
        .pick("id,linkedTo{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities = retrieved_2.body();
    expect_that!(entities.as_slice(), len(eq(2)));
    let item = entities.get(1).or_fail()?;
    expect_that!(item.entity_type, none());
    expect_that!(
        entities.as_slice(),
        nth(1, all!(has_id(&city_linked_identity_id), has_attribute("name", any_attribute())))
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_linked_identity_id)
        .join_level(1.try_into()?)
        .pick("id,linkedTo{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_2 = retrieved_3.body();
    let item_2 = entities_2.get(1).or_fail()?;
    expect_that!(item_2.id, none());
    expect_that!(item_2.entity_type, none());
    expect_that!(entities_2.as_slice(), nth(1, has_attribute("name", any_attribute())));
    Ok(())
}
