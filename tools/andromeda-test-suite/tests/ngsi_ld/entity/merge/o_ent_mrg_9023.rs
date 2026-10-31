//! A scope only merge after the history was deleted succeeds.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    spelling_matcher::alone,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange, Value, VocabProperty, VocabTerm},
};

/// Pins: Merge Entity with only scope /Madrid/Gardens on an Entity whose Temporal Evolution was deleted first
/// answers 204, setting the scope and leaving temperature 21.5
/// Clause: 4.18, 5.6.16.4, 5.6.17.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_ent_mrg_9023_a_scope_only_merge_after_the_history_was_deleted_succeeds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_scope_merge_id = namespace.urn("Building", "scope-merge")?;
    let building_type = namespace.entity_type("Building")?;
    let building_scope_merge = Entity::builder()
        .id(building_scope_merge_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_scope_merge).send().await?.done()?;
    broker.temporal().delete(&building_scope_merge_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_scope_merge_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/Madrid/Gardens".parse::<Scope>()?)))
        .build()?;
    broker.entities().merge(&building_scope_merge_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_scope_merge_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/Madrid/Gardens".parse::<Scope>()?)));
    expect_that!(entity, has_attribute("temperature", any_attribute().value(Value::Number("21.5".parse()?))));
    Ok(())
}
