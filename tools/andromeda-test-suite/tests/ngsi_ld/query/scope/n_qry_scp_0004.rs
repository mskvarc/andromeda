//! Slash hash matches every entity with a scope.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::scope_query::ScopeQuery,
};

/// Pins: /# matches every entity that has a scope, and never one without
/// Clause: 4.19, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_scp_0004_slash_hash_matches_every_entity_with_a_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .scope("/Madrid/Gardens/ParqueNorte")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .scope(["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"])
            .build()?,
        Entity::builder()
            .id(building_3_id)
            .entity_type(building_type.clone())
            .property("name", "unscoped")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Building 3 has no scope.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&building_2_id)));
    Ok(())
}
