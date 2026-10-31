//! A nested term deeper than join level is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{Join, projection::Projection},
};

/// Pins: a pick nesting two groups deep under `joinLevel=1` exceeds the Linked Entity retrieval depth and is
/// `BadRequestData` on Query Entities and on Retrieve Entity, and the same pick is answered once `joinLevel=2`
/// covers it
/// Clause: 5.7.1.4, 5.7.2.4, 4.21, 4.5.23
#[gtest]
#[tokio::test]
async fn n_lnk_prj_0006_a_nested_term_deeper_than_join_level_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_c_id = namespace.urn("Building", "c")?;
    let building_type = namespace.entity_type("Building")?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let entities = [
        Entity::builder()
            .id(building_c_id.clone())
            .entity_type(building_type.clone())
            .property("c", "deep")
            .build()?,
        Entity::builder()
            .id(building_b_id.clone())
            .entity_type(building_type.clone())
            .relationship("b", building_c_id.clone())
            .build()?,
        Entity::builder()
            .id(building_a_id.clone())
            .entity_type(building_type.clone())
            .relationship("a", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("a{b{c}}".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_a_id)
        .join_level(1.try_into()?)
        .pick("a{b{c}}".parse::<Projection>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // two hops cover the two groups: the root keeps a, the first Linked Entity b, the second c
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .pick("a{b{c}}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_attribute("a", any_attribute().object(building_b_id.clone()))));
    expect_that!(page.items(), nth(1, has_attribute("b", any_attribute().object(building_c_id.clone()))));
    expect_that!(page.items(), nth(2, has_attribute("c", any_attribute().value("deep"))));
    Ok(())
}
