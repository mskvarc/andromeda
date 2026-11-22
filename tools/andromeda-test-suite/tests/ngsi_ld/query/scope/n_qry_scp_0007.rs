//! A percent encoded plus in a scope query is the single level wildcard.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::Entity;

/// Pins: Query Entities with the query string `scopeQ=/Madrid/%2B/ParqueNorte` selects the entities in
/// /Madrid/Gardens/ParqueNorte and /Madrid/Sights/ParqueNorte and not the one in /Madrid/Gardens/ParqueSur
/// Clause: 4.9, 4.19, 5.7.2, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_scp_0007_a_percent_encoded_plus_in_a_scope_query_is_the_single_level_wildcard() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let gardens_id = namespace.urn("Building", 1)?;
    let sights_id = namespace.urn("Building", 2)?;
    let entities_written = [
        Entity::builder()
            .id(gardens_id.clone())
            .entity_type(building_type.clone())
            .scope("/Madrid/Gardens/ParqueNorte")
            .build()?,
        Entity::builder()
            .id(sights_id.clone())
            .entity_type(building_type.clone())
            .scope("/Madrid/Sights/ParqueNorte")
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", 3)?)
            .entity_type(building_type.clone())
            .scope("/Madrid/Gardens/ParqueSur")
            .build()?,
    ];
    broker.entities().batch_create(&entities_written).send().await?.done()?;
    // The typed query writes the scope query itself, so this read goes out as written, with the wildcard
    // percent-encoded as RFC 3986 section 2.2 has a reserved character sent as data.
    let type_text = building_type.to_string();
    let read = format!("{}&scopeQ=/Madrid/%2B/ParqueNorte", target("entities", &[("type", type_text.as_str())]));
    let answer = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let selected = entities(&answer?)?;
    expect_that!(selected, len(eq(2)));
    expect_that!(selected, contains(has_id(&gardens_id)));
    expect_that!(selected, contains(has_id(&sights_id)));
    Ok(())
}
