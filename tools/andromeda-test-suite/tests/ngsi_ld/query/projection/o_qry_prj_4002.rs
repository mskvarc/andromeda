//! Pick and omit apply both in the simplified representation and a core member in both is refused.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType, SimplifiedValue, Value},
    query::projection::Projection,
};

/// Pins: Query Entities with pick and omit naming different members answers 200 in the simplified representation
/// and in the normalized one, each Entity reduced to the picked members minus the omitted ones; a member named in
/// both, an Attribute in the simplified representation or the core member type, is refused with `BadRequestData`
/// Clause: 4.21, 5.7.2.4, 6.4.3.2, 4.5.1
/// Recorded: Table 6.4.3.2-1 defines pick and omit side by side and the clauses name no error for either
/// combination. Applying both when they name different members, and refusing a member named in both as a
/// self-contradictory projection with `BadRequestData`, is Andromeda's reading, so the file is kind O. Ruled by the
/// user (2026-10-05; the refusal of a member named in both 2026-10-11)
#[gtest]
#[tokio::test]
async fn o_qry_prj_4002_pick_and_omit_apply_both_in_the_simplified_representation_and_a_core_member_in_both_is_refused() -> Result<()> {
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
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let simplified = json
        .entities()
        .query_projected_simplified()
        .entity_type(&building_type)
        .pick("id,category".parse::<Projection>()?)
        .omit("name".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(simplified.items(), len(eq(1)));
    let item = simplified.items().first().or_fail()?;
    expect_that!(item.id.as_ref(), some(eq(&building_id)));
    expect_that!(item.entity_type, none());
    expect_that!(item.attributes, len(eq(1)));
    expect_that!(
        item.attributes.get(&"category".parse::<AttributeName>()?),
        some(eq(&SimplifiedValue::Value(Value::from("commercial"))))
    );
    let both = json
        .with_responses()
        .entities()
        .query_projected_simplified()
        .entity_type(&building_type)
        .pick("id,category,name".parse::<Projection>()?)
        .omit("name".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&both, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let normalized = json
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("id,category".parse::<Projection>()?)
        .omit("type".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(normalized.items(), len(eq(1)));
    let entity = normalized.items().first().or_fail()?;
    expect_that!(entity.id.as_ref(), some(eq(&building_id)));
    expect_that!(entity.entity_type, none());
    expect_that!(entity, has_attribute("category", property().value("commercial")));
    expect_that!(entity, lacks_attribute("name"));
    let both_2 = json
        .with_responses()
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("id,type,category".parse::<Projection>()?)
        .omit("type".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&both_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
