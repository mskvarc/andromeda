//! The scope is replaced by the new scopes.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{holding, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Scope},
};

/// Pins: Replace Entity of an entity with one scope by an entity with two scopes answers 204 and the entity reads
/// back with exactly the two new scopes
/// Clause: 5.6.18.4, 4.18, 6.5.3.3
#[gtest]
#[tokio::test]
async fn n_ent_rpl_0006_the_scope_is_replaced_by_the_new_scopes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let replacement = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope(["/Madrid/Gardens/ParqueSur", "/CompanyA/OrganizationB/UnitC"])
        .build()?;
    broker.entities().replace(&replacement).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/Madrid/Gardens/ParqueSur".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/CompanyA/OrganizationB/UnitC".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(not(holding(&"/Madrid/Gardens/ParqueNorte".parse::<Scope>()?))));
    Ok(())
}
