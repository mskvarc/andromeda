//! A percent encoded plus in a q string is a plus.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::Entity;

/// Pins: Query Entities with the query string `q=name==%22a%2Bb%22` selects the Building whose name is `a+b` and
/// not the one whose name is `a b`
/// Clause: 4.9, 5.7.2, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9090_a_percent_encoded_plus_in_a_q_string_is_a_plus() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let plus_id = namespace.urn("Building", 2)?;
    let buildings = [
        Entity::builder()
            .id(namespace.urn("Building", 1)?)
            .entity_type(building_type.clone())
            .property("name", "a b")
            .build()?,
        Entity::builder()
            .id(plus_id.clone())
            .entity_type(building_type.clone())
            .property("name", "a+b")
            .build()?,
    ];
    broker.entities().batch_create(&buildings).send().await?.done()?;
    // The typed query writes the q string itself, so this read goes out as written, with the plus percent-encoded as
    // RFC 3986 section 2.2 has a reserved character sent as data.
    let type_text = building_type.to_string();
    let read = format!("{}&q=name==%22a%2Bb%22", target("entities", &[("type", type_text.as_str())]));
    let answer = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let selected = entities(&answer?)?;
    expect_that!(selected, len(eq(1)));
    expect_that!(selected, nth(0, has_id(&plus_id)));
    Ok(())
}
