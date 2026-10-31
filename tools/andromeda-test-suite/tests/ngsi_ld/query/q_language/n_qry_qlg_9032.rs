//! A surrogate pair matches one supplementary character.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
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
        .entity_type(escape_type.clone())
        .property("name", "clef-𝄞")
        .build()?;
    broker.entities().create(&escape_clef).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_clef_id.clone()])
        .q(q::attr("name").eq("clef-𝄞"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&escape_clef_id)));
    Ok(())
}
