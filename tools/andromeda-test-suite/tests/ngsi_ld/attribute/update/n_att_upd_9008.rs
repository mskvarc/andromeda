//! A multi level scope is written by every write verb.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, spelling_matcher::alone};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange, Value},
};

/// Pins: A scope /a/b/c written by Create Entity reads back, and is replaced by /a/b/d through Append Entity
/// Attributes, by /a/b/e through Update Entity Attributes and by /a/b/c again through Merge Entity
/// Clause: 4.18, 5.6.2.4, 5.6.3.4, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_att_upd_9008_a_multi_level_scope_is_written_by_every_write_verb() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_verbs_id = namespace.urn("Building", "verbs")?;
    let building_type = namespace.entity_type("Building")?;
    let building_verbs = Entity::builder()
        .id(building_verbs_id.clone())
        .entity_type(building_type)
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_verbs).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_verbs_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/a/b/c".parse::<Scope>()?)));
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/a/b/d".parse::<Scope>()?)))
        .build()?;
    broker.entities().append_attributes(&building_verbs_id, &fragment).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_verbs_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.scope.as_ref(), some(alone(&"/a/b/d".parse::<Scope>()?)));
    let fragment_2 = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/a/b/e".parse::<Scope>()?)))
        .build()?;
    broker.entities().update_attributes(&building_verbs_id, &fragment_2).send().await?.done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_verbs_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.scope.as_ref(), some(alone(&"/a/b/e".parse::<Scope>()?)));
    let fragment_3 = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/a/b/c".parse::<Scope>()?)))
        .build()?;
    broker.entities().merge(&building_verbs_id, &fragment_3).send().await?.done()?;
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_verbs_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4.scope.as_ref(), some(alone(&"/a/b/c".parse::<Scope>()?)));
    Ok(())
}
