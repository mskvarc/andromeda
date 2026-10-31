//! Two scopes read back as an array of both.

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

/// Pins: an entity created with two scopes reads back with both scopes in an array
/// Clause: 5.6.1, 4.18, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0008_two_scopes_read_back_as_an_array_of_both() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope(["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"])
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.scope.as_ref(), some(listing_count(2)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/Madrid/Gardens/ParqueNorte".parse::<Scope>()?)));
    expect_that!(entity.scope.as_ref(), some(holding(&"/CompanyA/OrganizationB/UnitC".parse::<Scope>()?)));
    Ok(())
}
