//! Pick keeps only listed members and omit removes only listed ones.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute, projected_member_count},
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, OneOrMany, Property, Scope},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick reduces the entity to exactly the listed members, keeping id or type only when
/// listed and scope or system members never unless listed, and omit=name drops name alone
/// Clause: 4.21, 5.7.1.5, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0026_pick_keeps_only_listed_members_and_omit_removes_only_listed_ones() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .property(
                "airQualityLevel",
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?),
            )
            .property("almostFull", false)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .scope("/Madrid")
            .property("category", "commercial")
            .property("name", "Depot")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json
        .entities()
        .retrieve_projected(&building_id)
        .pick("name".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(projected_member_count(entity), eq(1));
    expect_that!(entity, has_attribute("name", property().value("Eiffel Tower")));
    expect_that!(entity.id, none());
    expect_that!(entity.entity_type, none());
    expect_that!(entity.output.created_at, none());
    let retrieved_2 = json
        .entities()
        .retrieve_projected(&building_id)
        .pick("id".parse::<Projection>()?)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(projected_member_count(entity_2), eq(1));
    expect_that!(entity_2.id, some(eq(&building_id)));
    expect_that!(entity_2.entity_type, none());
    expect_that!(entity_2, lacks_attribute("name"));
    let retrieved_3 = json
        .entities()
        .retrieve_projected(&building_id)
        .pick("id,name".parse::<Projection>()?)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(projected_member_count(entity_3), eq(2));
    expect_that!(entity_3.id, some(eq(&building_id)));
    expect_that!(entity_3, has_attribute("name", property().value("Eiffel Tower")));
    expect_that!(entity_3.entity_type, none());
    expect_that!(entity_3, lacks_attribute("subCategory"));
    // pick=category,id keeps id and category, drops name, type and scope
    let retrieved_4 = json
        .entities()
        .retrieve_projected(&building_2_id)
        .pick("category,id".parse::<Projection>()?)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(projected_member_count(entity_4), eq(2));
    expect_that!(entity_4.id, some(eq(&building_2_id)));
    expect_that!(entity_4, has_attribute("category", property().value("commercial")));
    expect_that!(entity_4, lacks_attribute("name"));
    expect_that!(entity_4.entity_type, none());
    expect_that!(entity_4.scope, none());
    // omit=name drops name and keeps every other member
    let retrieved_5 = json
        .entities()
        .retrieve_projected(&building_2_id)
        .omit("name".parse::<Projection>()?)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(projected_member_count(entity_5), eq(4));
    expect_that!(entity_5.id, some(eq(&building_2_id)));
    expect_that!(entity_5.entity_type, some(eq(&OneOrMany::one(building_type))));
    expect_that!(entity_5.scope, some(eq(&OneOrMany::one("/Madrid".parse::<Scope>()?))));
    expect_that!(entity_5, has_attribute("category", property().value("commercial")));
    expect_that!(entity_5, lacks_attribute("name"));
    Ok(())
}
