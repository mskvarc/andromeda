//! A merge sets a scope on an entity without one.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{holding, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange, Value},
};

/// Pins: Merge Entity with scope `["/A/B","/C/D"]` onto an Entity without a scope leaves exactly /A/B and /C/D
/// Clause: 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9013_a_merge_sets_a_scope_on_an_entity_without_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_absent_id = namespace.urn("Building", "absent")?;
    let building_type = namespace.entity_type("Building")?;
    let building_absent = Entity::builder()
        .id(building_absent_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_absent).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::try_from(vec!["/A/B".parse::<Scope>()?, "/C/D".parse::<Scope>()?])?))
        .build()?;
    broker.entities().merge(&building_absent_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_absent_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/A/B".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/C/D".parse::<Scope>()?)));
    Ok(())
}
