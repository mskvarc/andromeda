//! A unicode escape matches the code point and not its letters.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
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
        .id(escape_letters_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "au00e9")
        .build()?;
    broker.entities().create(&escape_letters).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_accented_id.clone(), escape_letters_id.clone()])
        .q(q::attr("name").eq("aé"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&escape_accented_id)));
    Ok(())
}
