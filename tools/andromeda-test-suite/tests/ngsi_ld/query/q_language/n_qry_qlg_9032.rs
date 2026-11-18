//! A surrogate pair matches one supplementary character.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::answered, unchecked_page, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities with name=="clef-\ud834\udd1e" answers the Entity whose name is clef- followed by U+1D11E
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9032_a_surrogate_pair_matches_one_supplementary_character() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let escape_clef_id = namespace.urn("Escape", "clef")?;
    let escape_type = namespace.entity_type("Escape")?;
    let escape_clef = Entity::builder()
        .id(escape_clef_id.clone())
        .entity_type(escape_type)
        .property("name", "clef-𝄞")
        .build()?;
    broker.entities().create(&escape_clef).send().await?.done()?;
    // The model writes the character a surrogate pair names rather than the pair, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Escape_{namespace}&id=urn%3Angsi-ld%3AEscape%3A{namespace}-clef&q=name%3D%3D%22clef-%5Cud834%5Cudd1e%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let page = unchecked_page::entities(&answer?)?;
    expect_that!(page, len(eq(1)));
    expect_that!(page, contains(has_id(&escape_clef_id)));
    Ok(())
}
