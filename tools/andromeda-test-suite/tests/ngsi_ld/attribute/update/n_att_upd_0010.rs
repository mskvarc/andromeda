//! A scope replaces the scope of the entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::alone,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange, Value},
};

/// Pins: Update Attributes with a scope on an Entity that has a scope answers 204 and the Entity scope becomes the
/// one of the Fragment
/// Clause: 5.6.2.4, 4.18
#[gtest]
#[tokio::test]
async fn n_att_upd_0010_a_scope_replaces_the_scope_of_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope("/Madrid/Gardens/ParqueNorte")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/Madrid/Gardens/ParqueNorte".parse::<Scope>()?)));
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/CompanyA/OrganizationB/UnitC".parse::<Scope>()?)))
        .build()?;
    broker.entities().update_attributes(&building_id, &fragment).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.scope.as_ref(), some(alone(&"/CompanyA/OrganizationB/UnitC".parse::<Scope>()?)));
    expect_that!(entity_2, has_attribute("temperature", any_attribute().value(Value::Number("21.5".parse()?))));
    Ok(())
}
