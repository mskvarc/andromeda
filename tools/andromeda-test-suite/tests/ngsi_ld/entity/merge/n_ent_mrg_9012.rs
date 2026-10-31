//! An overlapping scope array replaces the scope whole.

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

/// Pins: Merge Entity with scope `["/X/A","/Q/R"]` onto scope `["/X/A","/Y/A"]` leaves exactly /X/A and /Q/R, which
/// `scopeQ` selects while /Y/A selects nothing
/// Clause: 4.18, 4.19, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9012_an_overlapping_scope_array_replaces_the_scope_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_overlap_id = namespace.urn("Building", "overlap")?;
    let building_type = namespace.entity_type("Building")?;
    let building_overlap = Entity::builder()
        .id(building_overlap_id.clone())
        .entity_type(building_type.clone())
        .scope(["/X/A", "/Y/A"])
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_overlap).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::try_from(vec!["/X/A".parse::<Scope>()?, "/Q/R".parse::<Scope>()?])?))
        .build()?;
    broker.entities().merge(&building_overlap_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_overlap_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/X/A".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/Q/R".parse::<Scope>()?)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_overlap_id.clone()])
        .scope_q("/X/A".parse::<ScopeQuery>()?)
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
        .ids(vec![building_overlap_id.clone()])
        .scope_q("/Q/R".parse::<ScopeQuery>()?)
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
        .ids(vec![building_overlap_id.clone()])
        .scope_q("/Y/A".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(0)));
    Ok(())
}
