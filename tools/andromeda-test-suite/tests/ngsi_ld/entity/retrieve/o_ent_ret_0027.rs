//! Pick and omit apply both and a member in both is refused.

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
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with pick and omit naming different members answers 200 with the Entity reduced to the
/// picked members minus the omitted ones; a member named in both is refused with `BadRequestData`
/// Clause: 4.21, 5.7.1.4, 6.5.3.1
/// Recorded: Table 6.5.3.1-1 defines pick and omit side by side and the clauses name no error for either
/// combination. Applying both when they name different members, and refusing a member named in both as a
/// self-contradictory projection with `BadRequestData`, is Andromeda's reading, so the file is kind O. Ruled by the
/// user (2026-10-05; the refusal of a member named in both 2026-10-11)
#[gtest]
#[tokio::test]
async fn o_ent_ret_0027_pick_and_omit_apply_both_and_a_member_in_both_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("category", "commercial")
        .property("name", "Depot")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .pick("category".parse::<Projection>()?)
        .omit("name".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("name"));
    expect_that!(entity, has_attribute("category", any_attribute().value("commercial")));
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_id)
        .pick("category,name".parse::<Projection>()?)
        .omit("name".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
