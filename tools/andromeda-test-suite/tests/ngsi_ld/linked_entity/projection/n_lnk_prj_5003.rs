//! Query body omit through a linked entity term leaves the unnamed second hop whole.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    type_matcher::{has_type, no_type},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType, OneOrMany},
    query::{Join, projection::Projection},
};

/// Pins: Query Entities by POST `/entityOperations/query` with join, `joinLevel` and an omit or a pick member
/// carrying a Linked Entity term answers as the GET form does: the term reaches the Linked Entity its Relationship
/// names only, and the Entity reached through an unnamed second Relationship is returned whole, in inline and in
/// flat form
/// Clause: 4.21, 4.5.23, 5.2.8, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_prj_5003_query_body_omit_through_a_linked_entity_term_leaves_the_unnamed_second_hop_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_id = namespace.urn("City", 1)?;
    let city_type = namespace.entity_type("City")?;
    let country_id = namespace.urn("Country", 1)?;
    let country_type = namespace.entity_type("Country")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .relationship("locatedAt", city_id.clone())
            .build()?,
        Entity::builder()
            .id(city_id.clone())
            .entity_type(city_type.clone())
            .property("name", "Paris")
            .relationship("isInCountry", country_id.clone())
            .build()?,
        Entity::builder()
            .id(country_id.clone())
            .entity_type(country_type.clone())
            .property("name", "France")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // inline, omit
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .omit("type,locatedAt{name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(item.entity_type, none());
    expect_that!(
        linked_entities(item, &["locatedAt"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&city_type))
    );
    expect_that!(linked_entities(item, &["locatedAt"]), nth(0, lacks_attribute("name")));
    expect_that!(
        linked_entities(item, &["locatedAt", "isInCountry"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&country_type))
    );
    expect_that!(
        linked_entities(item, &["locatedAt", "isInCountry"]),
        nth(0, has_attribute("name", any_attribute().value("France")))
    );
    expect_that!(
        page.items(),
        nth(0, all!(has_id(&building_id), has_attribute("name", any_attribute().value("Eiffel Tower"))))
    );
    // flat, omit
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .omit("type,locatedAt{name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(
        page_2.items(),
        contains(all!(
            has_id(&building_id),
            has_attribute("name", any_attribute().value("Eiffel Tower")),
            no_type()
        ))
    );
    expect_that!(page_2.items(), contains(all!(has_id(&city_id), lacks_attribute("name"), has_type(&city_type))));
    expect_that!(
        page_2.items(),
        contains(all!(
            has_id(&country_id),
            has_attribute("name", any_attribute().value("France")),
            has_type(&country_type)
        ))
    );
    // inline, pick naming the first Relationship and the second one as a plain member
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .pick("id,locatedAt{name,isInCountry}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    let item_2 = page_3.items().first().or_fail()?;
    expect_that!(
        linked_entities(item_2, &["locatedAt"]),
        nth(0, has_attribute("name", any_attribute().value("Paris")))
    );
    expect_that!(
        linked_entities(item_2, &["locatedAt"]).first().and_then(|linked| linked.entity_type.as_ref()),
        none()
    );
    expect_that!(
        linked_entities(item_2, &["locatedAt", "isInCountry"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&country_type))
    );
    expect_that!(
        linked_entities(item_2, &["locatedAt", "isInCountry"]),
        nth(0, has_attribute("name", any_attribute().value("France")))
    );
    expect_that!(page_3.items(), nth(0, all!(has_id(&building_id), lacks_attribute("name"))));
    Ok(())
}
