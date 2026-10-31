//! A single scope replaces a scope array.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, spelling_matcher::alone};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange, Value},
    query::scope_query::ScopeQuery,
};

/// Pins: Merge Entity with scope "/G/H" onto scope `["/X/A","/Y/A","/Z/A"]` leaves the scope /G/H alone, which
/// `scopeQ` selects while /Y/A selects nothing
/// Clause: 4.18, 4.19, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9011_a_single_scope_replaces_a_scope_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_string_id = namespace.urn("Building", "string")?;
    let building_type = namespace.entity_type("Building")?;
    let building_string = Entity::builder()
        .id(building_string_id.clone())
        .entity_type(building_type.clone())
        .scope(["/X/A", "/Y/A", "/Z/A"])
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_string).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/G/H".parse::<Scope>()?)))
        .build()?;
    broker.entities().merge(&building_string_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_string_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/G/H".parse::<Scope>()?)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_string_id.clone()])
        .scope_q("/G/H".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_string_id.clone()])
        .scope_q("/Y/A".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(0)));
    Ok(())
}
