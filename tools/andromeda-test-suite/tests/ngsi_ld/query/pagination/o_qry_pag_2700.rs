//! Query entities in cursor mode answers one link line with the context link first.

use andromeda_test_suite::{
    broker,
    link_matcher::relations_in_order,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::{entities, follow},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
    },
    model::Entity,
};

/// Pins: Query Entities in cursor mode over several pages carries one Link field line holding the @context link
/// first, then first, prev, next and last
/// Clause: 6.3.6, 6.3.10, 5.5.9.1
/// Recorded: Andromeda answers the Link field on one line; RFC 9110 clause 5.3 makes one combined line and several
/// lines equivalent, and the test pins the single line
#[gtest]
#[tokio::test]
async fn o_qry_pag_2700_query_entities_in_cursor_mode_answers_one_link_line_with_the_context_link_first() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let mut entities_created = Vec::new();
    for n in 1..=3 {
        entities_created.push(Entity::builder().id(namespace.urn("Building", n)?).entity_type(building_type.clone()).build()?);
    }
    broker.entities().batch_create(&entities_created).send().await?.done()?;
    // The cursor parameter is the broker's own continuation, which the typed parameter models do not know.
    let type_text = building_type.to_string();
    let read = target("entities", &[("type", type_text.as_str()), ("limit", "1"), ("cursor", "")]);
    let first = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
        .await;
    expect_that!(&first, answered(StatusCode::OK));
    let first = first?;
    expect_that!(entities(&first)?, len(eq(1)));
    let second = follow(&broker, &first, &RelationType::Next).await?.or_fail()?;
    expect_that!(&second, answered(StatusCode::OK));
    let second = second?;
    expect_that!(entities(&second)?, len(eq(1)));
    expect_that!(
        ResponseHead::new(second.headers().clone()).typed::<Link>().valid(),
        some(relations_in_order([
            RelationType::JsonLdContext,
            RelationType::First,
            RelationType::Prev,
            RelationType::Next,
            RelationType::Last
        ]))
    );
    Ok(())
}
