//! An escaped tab in the same shape is admitted.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with name=="a\tb" answers 200 with the Entity whose name holds a tab between a and b
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9035_an_escaped_tab_in_the_same_shape_is_admitted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let escape_tab_id = namespace.urn("Escape", "tab")?;
    let escape_type = namespace.entity_type("Escape")?;
    let escape_tab = Entity::builder()
        .id(escape_tab_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "a\tb")
        .build()?;
    broker.entities().create(&escape_tab).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_tab_id.clone()])
        .q(q::attr("name").eq("a\tb"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&escape_tab_id)));
    Ok(())
}
