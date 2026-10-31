//! Pick and omit apply both in the simplified representation and a core member in both is refused.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType, SimplifiedValue, Value},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick and omit naming different members answers 200 in the simplified representation
/// and in the normalized one, the Entity reduced to the picked members minus the omitted ones; a member named in
/// both, an Attribute in the simplified representation or the core member type, is refused with `BadRequestData`
/// Clause: 4.21, 5.7.1.4, 6.5.3.1, 4.5.1
/// Recorded: Table 6.5.3.1-1 defines pick and omit side by side and the clauses name no error for either
/// combination. Applying both when they name different members, and refusing a member named in both as a
/// self-contradictory projection with `BadRequestData`, is Andromeda's reading, so the file is kind O. Ruled by the
/// user (2026-10-05; the refusal of a member named in both 2026-10-11)
#[gtest]
#[tokio::test]
async fn o_ent_ret_4001_pick_and_omit_apply_both_in_the_simplified_representation_and_a_core_member_in_both_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("category", "commercial")
        .property("name", "Depot")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let category = "category".parse::<AttributeName>()?;
    let name = "name".parse::<AttributeName>()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected_simplified(&building_id)
        .pick("id,category".parse::<Projection>()?)
        .omit("name".parse::<Projection>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, some(eq(&building_id)));
    expect_that!(entity.attributes.get(&category), some(eq(&SimplifiedValue::Value(Value::from("commercial")))));
    expect_that!(entity.attributes.get(&name), none());
    expect_that!(entity.entity_type, none());
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected_simplified(&building_id)
        .pick("id,category,name".parse::<Projection>()?)
        .omit("name".parse::<Projection>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .pick("id,category".parse::<Projection>()?)
        .omit("type".parse::<Projection>()?)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, some(eq(&building_id)));
    expect_that!(entity_2, has_attribute("category", any_attribute().value("commercial")));
    expect_that!(entity_2.entity_type, none());
    expect_that!(entity_2, lacks_attribute("name"));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .pick("id,type,category".parse::<Projection>()?)
        .omit("type".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
