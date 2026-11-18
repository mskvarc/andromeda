//! A unicode escape matches the code point and not its letters.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::answered, unchecked_page, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities with name=="a\u00e9" answers the Entity named aé and not the one named au00e9
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9031_a_unicode_escape_matches_the_code_point_and_not_its_letters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let escape_accented_id = namespace.urn("Escape", "accented")?;
    let escape_type = namespace.entity_type("Escape")?;
    let escape_letters_id = namespace.urn("Escape", "letters")?;
    let escape_accented = Entity::builder()
        .id(escape_accented_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "aé")
        .build()?;
    broker.entities().create(&escape_accented).send().await?.done()?;
    let escape_letters = Entity::builder()
        .id(escape_letters_id)
        .entity_type(escape_type)
        .property("name", "au00e9")
        .build()?;
    broker.entities().create(&escape_letters).send().await?.done()?;
    // The model writes the character an escape names rather than the escape, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "entities?type=Escape_{namespace}&id=urn%3Angsi-ld%3AEscape%3A{namespace}-accented,urn%3Angsi-ld%3AEscape%3A{namespace}-letters&q=name%3D%3D%22a%5Cu00e9%22"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let page = unchecked_page::entities(&answer?)?;
    expect_that!(page, len(eq(1)));
    expect_that!(page, contains(has_id(&escape_accented_id)));
    Ok(())
}
