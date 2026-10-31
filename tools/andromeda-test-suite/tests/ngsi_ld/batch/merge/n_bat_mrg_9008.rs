//! Every shape of scope is merged as merge entity merges it.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{alone, holding, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange, Value},
};

/// Pins: Batch Entity Merge of four Entities answers 204 and leaves the scope array replaced by an array, the array
/// replaced by a single scope, a scope set where there was none and the scope deleted by the null
/// Clause: 4.18, 5.6.17.4, 5.6.20.4
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9008_every_shape_of_scope_is_merged_as_merge_entity_merges_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_array_id = namespace.urn("Building", "batch-array")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch_string_id = namespace.urn("Building", "batch-string")?;
    let building_batch_absent_id = namespace.urn("Building", "batch-absent")?;
    let building_batch_null_id = namespace.urn("Building", "batch-null")?;
    let building_batch_array = Entity::builder()
        .id(building_batch_array_id.clone())
        .entity_type(building_type.clone())
        .scope(["/X/A", "/Y/A", "/Z/A"])
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_batch_array).send().await?.done()?;
    let building_batch_string = Entity::builder()
        .id(building_batch_string_id.clone())
        .entity_type(building_type.clone())
        .scope(["/X/A", "/Y/A", "/Z/A"])
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_batch_string).send().await?.done()?;
    let building_batch_absent = Entity::builder()
        .id(building_batch_absent_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_batch_absent).send().await?.done()?;
    let building_batch_null = Entity::builder()
        .id(building_batch_null_id.clone())
        .entity_type(building_type.clone())
        .scope(["/X/A", "/Y/A"])
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_batch_null).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_batch_array_id.clone(), building_type.clone())
            .scope(ScopeChange::Set(OneOrMany::try_from(vec!["/A/B".parse::<Scope>()?, "/C/D".parse::<Scope>()?])?))
            .build()?,
        EntityFragment::for_entity(building_batch_string_id.clone(), building_type.clone())
            .scope(ScopeChange::Set(OneOrMany::one("/G/H".parse::<Scope>()?)))
            .build()?,
        EntityFragment::for_entity(building_batch_absent_id.clone(), building_type.clone())
            .scope(ScopeChange::Set(OneOrMany::try_from(vec!["/A/B".parse::<Scope>()?, "/C/D".parse::<Scope>()?])?))
            .build()?,
        EntityFragment::for_entity(building_batch_null_id.clone(), building_type.clone())
            .scope(ScopeChange::Delete)
            .build()?,
    ];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_array_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/A/B".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/C/D".parse::<Scope>()?)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_string_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.scope.as_ref(), some(alone(&"/G/H".parse::<Scope>()?)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_absent_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity_3.scope.as_ref(), some(holding(&"/A/B".parse::<Scope>()?)));
    expect_that!(entity_3.scope.as_ref(), some(holding(&"/C/D".parse::<Scope>()?)));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_null_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4.scope.as_ref(), none());
    Ok(())
}
