//! Retrieve omit through a linked entity term leaves the unnamed second hop whole.

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
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType, OneOrMany},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with `joinLevel=2` and an omit term whose group names one Relationship (`locatedAt{name}`)
/// removes name from the Linked Entity that Relationship reaches only: the Entity reached through the next, unnamed
/// Relationship (`isInCountry`) is returned whole, in inline and in flat form, and with the same term in a pick
/// (locatedAt{name,isInCountry}) the unnamed Country is whole as well
/// Clause: 4.21, 4.5.23, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_lnk_prj_5002_retrieve_omit_through_a_linked_entity_term_leaves_the_unnamed_second_hop_whole() -> Result<()> {
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
    // inline, omit=type,locatedAt{name}
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .omit("type,locatedAt{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, some(eq(&building_id)));
    expect_that!(entity.entity_type, none());
    expect_that!(linked_entities(entity, &["locatedAt"]), nth(0, has_id(&city_id)));
    expect_that!(
        linked_entities(entity, &["locatedAt"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&city_type))
    );
    expect_that!(linked_entities(entity, &["locatedAt"]), nth(0, lacks_attribute("name")));
    expect_that!(linked_entities(entity, &["locatedAt", "isInCountry"]), nth(0, has_id(&country_id)));
    expect_that!(
        linked_entities(entity, &["locatedAt", "isInCountry"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&country_type))
    );
    expect_that!(
        linked_entities(entity, &["locatedAt", "isInCountry"]),
        nth(0, has_attribute("name", any_attribute().value("France")))
    );
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    // flat, omit=type,locatedAt{name}
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_id)
        .join_level(2.try_into()?)
        .omit("type,locatedAt{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body();
    expect_that!(entities_2.as_slice(), len(eq(3)));
    expect_that!(
        entities_2.as_slice(),
        contains(all!(
            has_id(&building_id),
            has_attribute("name", any_attribute().value("Eiffel Tower")),
            no_type()
        ))
    );
    expect_that!(
        entities_2.as_slice(),
        contains(all!(
            has_id(&city_id),
            has_attribute("isInCountry", any_attribute().object(country_id.clone())),
            lacks_attribute("name"),
            has_type(&city_type)
        ))
    );
    expect_that!(
        entities_2.as_slice(),
        contains(all!(
            has_id(&country_id),
            has_attribute("name", any_attribute().value("France")),
            has_type(&country_type)
        ))
    );
    // inline, pick=id,locatedAt{name,isInCountry}: the City is reduced to name and the Relationship, no term addresses the Country
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .pick("id,locatedAt{name,isInCountry}".parse::<Projection>()?)
        .send()
        .await?;
    let entity_2 = retrieved_3.body();
    expect_that!(entity_2.id, some(eq(&building_id)));
    expect_that!(entity_2, lacks_attribute("name"));
    expect_that!(
        linked_entities(entity_2, &["locatedAt"]),
        nth(0, has_attribute("name", any_attribute().value("Paris")))
    );
    expect_that!(
        linked_entities(entity_2, &["locatedAt"]).first().and_then(|linked| linked.entity_type.as_ref()),
        none()
    );
    expect_that!(
        linked_entities(entity_2, &["locatedAt"]),
        nth(0, has_attribute("isInCountry", any_attribute().object(country_id.clone())))
    );
    expect_that!(linked_entities(entity_2, &["locatedAt", "isInCountry"]), nth(0, has_id(&country_id)));
    expect_that!(
        linked_entities(entity_2, &["locatedAt", "isInCountry"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&country_type))
    );
    expect_that!(
        linked_entities(entity_2, &["locatedAt", "isInCountry"]),
        nth(0, has_attribute("name", any_attribute().value("France")))
    );
    // omit through two terms: the nested group names the second Relationship, its target loses name, the City keeps it
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .omit("locatedAt{isInCountry{name}}".parse::<Projection>()?)
        .send()
        .await?;
    let entity_3 = retrieved_4.body();
    expect_that!(entity_3.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(
        linked_entities(entity_3, &["locatedAt"]),
        nth(0, has_attribute("name", any_attribute().value("Paris")))
    );
    expect_that!(
        linked_entities(entity_3, &["locatedAt"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&city_type))
    );
    expect_that!(linked_entities(entity_3, &["locatedAt", "isInCountry"]), nth(0, has_id(&country_id)));
    expect_that!(
        linked_entities(entity_3, &["locatedAt", "isInCountry"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&country_type))
    );
    expect_that!(linked_entities(entity_3, &["locatedAt", "isInCountry"]), nth(0, lacks_attribute("name")));
    Ok(())
}
