//! A scope array replaces a longer scope array.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{holding, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange, Value},
    query::scope_query::ScopeQuery,
};

/// Pins: Merge Entity with scope `["/A/B","/C/D"]` onto scope `["/X/A","/Y/A","/Z/A"]` leaves exactly /A/B and
/// /C/D, which `scopeQ` selects while /X/A, /Y/A and /Z/A select nothing
/// Clause: 4.18, 4.19, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9010_a_scope_array_replaces_a_longer_scope_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_array_id = namespace.urn("Building", "array")?;
    let building_type = namespace.entity_type("Building")?;
    let building_array = Entity::builder()
        .id(building_array_id.clone())
        .entity_type(building_type.clone())
        .scope(["/X/A", "/Y/A", "/Z/A"])
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_array).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::try_from(vec!["/A/B".parse::<Scope>()?, "/C/D".parse::<Scope>()?])?))
        .build()?;
    broker.entities().merge(&building_array_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_array_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/A/B".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/C/D".parse::<Scope>()?)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_array_id.clone()])
        .scope_q("/A/B".parse::<ScopeQuery>()?)
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
        .ids(vec![building_array_id.clone()])
        .scope_q("/C/D".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_array_id.clone()])
        .scope_q("/X/A".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_array_id.clone()])
        .scope_q("/Y/A".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(0)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_array_id.clone()])
        .scope_q("/Z/A".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(0)));
    Ok(())
}
