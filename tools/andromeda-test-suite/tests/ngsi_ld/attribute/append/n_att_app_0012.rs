//! No overwrite adds the scope beside the existing one.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::{holding, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Scope, ScopeChange},
};

/// Pins: Append Attributes with `options=noOverwrite` and a scope the Entity lacks answers 204 and the Entity scope
/// becomes an array holding both scopes
/// Clause: 5.6.3.4, 4.18
#[gtest]
#[tokio::test]
async fn n_att_app_0012_no_overwrite_adds_the_scope_beside_the_existing_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one("/CompanyA/OrganizationB/UnitC".parse::<Scope>()?)))
        .build()?;
    broker
        .entities()
        .append_attributes(&building_id, &fragment)
        .overwrite(Overwrite::Keep)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/Madrid/Gardens/ParqueNorte".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/CompanyA/OrganizationB/UnitC".parse::<Scope>()?)));
    Ok(())
}
