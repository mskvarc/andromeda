//! A post query with pick and omit applies both and a member in both is refused.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Query Entities by POST with a pick and an omit member naming different members answers 200 with each
/// Entity reduced to the picked members minus the omitted ones; a body naming a member in both is refused with
/// `BadRequestData`
/// Clause: 4.21, 5.2.23, 5.7.2.4, 6.23.3.1
/// Recorded: Table 5.2.23-1 defines pick and omit side by side and the clauses name no error for either
/// combination. Applying both when they name different members, and refusing a member named in both as a
/// self-contradictory projection with `BadRequestData`, is Andromeda's reading, so the file is kind O. Ruled by the
/// user (2026-10-05; the refusal of a member named in both 2026-10-11)
#[gtest]
#[tokio::test]
async fn o_qry_prj_4001_a_post_query_with_pick_and_omit_applies_both_and_a_member_in_both_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .property("category", "commercial")
        .property("name", "Depot")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .pick("category".parse::<Projection>()?)
        .omit("name".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(0, all!(has_attribute("category", any_attribute().value("commercial")), lacks_attribute("name")))
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .pick("id,category,name".parse::<Projection>()?)
        .omit("id,name".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
