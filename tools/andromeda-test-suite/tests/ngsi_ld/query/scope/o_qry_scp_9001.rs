//! A rooted pattern selects an entity scope without the slash.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, spelling_matcher::alone};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Scope},
    query::scope_query::ScopeQuery,
};

/// Pins: Query Entities with `scopeQ=/T<ns>` answers the Entity whose scope is T<ns>, with its scope as written
/// Clause: 4.18, 4.19, 5.7.2
/// Recorded: 4.18 makes the leading slash of a Scope optional ("Madrid" is its example 2) and 4.19 writes every
/// pattern with one; Andromeda reads a Scope written without the slash as the same levels, so a rooted pattern
/// selects it
#[gtest]
#[tokio::test]
async fn o_qry_scp_9001_a_rooted_pattern_selects_an_entity_scope_without_the_slash() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_bare_id = namespace.urn("Building", "bare")?;
    let building_type = namespace.entity_type("Building")?;
    let building_bare = Entity::builder()
        .id(building_bare_id.clone())
        .entity_type(building_type.clone())
        .scope(format!("T{namespace}").as_str())
        .build()?;
    broker.entities().create(&building_bare).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_bare_id.clone()])
        .scope_q(format!("/T{namespace}").as_str().parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(item.scope.as_ref(), some(alone(&format!("T{namespace}").parse::<Scope>()?)));
    expect_that!(page.items(), nth(0, has_id(&building_bare_id)));
    Ok(())
}
