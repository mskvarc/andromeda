//! An empty cursor starts a cursor walk.

use andromeda_test_suite::{
    broker,
    identity_matcher::ids_in_order,
    link_header::link_text,
    namespace::Namespace,
    outcome::Done,
    raw_call::{call, target},
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::Entity;

/// Pins: Query Entities with limit 2 and an empty cursor over four entities answers the first two with a next
/// pointer carrying a cursor and no offset
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn a_qry_pag_0033_an_empty_cursor_starts_a_cursor_walk() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    for number in 0..4 {
        let building = Entity::builder()
            .id(namespace.urn("Building", format!("{number:04}"))?)
            .entity_type(building_type.clone())
            .property("name", format!("b{number}"))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    // An empty `cursor` is a parameter the query model does not write, so the query is sent as written.
    let building_type_text = building_type.to_string();
    let answer = call(
        &broker,
        Method::GET,
        &target(
            "entities",
            &[("type", building_type_text.as_str()), ("local", "true"), ("limit", "2"), ("cursor", "")],
        ),
        &[("Accept", "application/ld+json")],
        None,
    )
    .await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(
        entities(&answer)?.as_slice(),
        ids_in_order(&[namespace.urn("Building", "0000")?, namespace.urn("Building", "0001")?])
    );
    let links = link_text(answer.headers());
    expect_that!(links, contains_substring("rel=\"next\""));
    expect_that!(links, contains_substring("cursor="));
    expect_that!(links, not(contains_substring("offset=")));
    Ok(())
}
