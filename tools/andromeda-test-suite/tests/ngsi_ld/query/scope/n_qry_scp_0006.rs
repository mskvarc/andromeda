//! A disjunction selects entities in any scope.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::scope_query::ScopeQuery,
};

/// Pins: A scope disjunction, written with , or |, selects the entities in any of the scopes it names
/// Clause: 4.19, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_scp_0006_a_disjunction_selects_entities_in_any_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let area_a_id = namespace.urn("Area", "a")?;
    let area_type = namespace.entity_type("Area")?;
    let area_b_id = namespace.urn("Area", "b")?;
    let area_c_id = namespace.urn("Area", "c")?;
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
            .id(area_a_id.clone())
            .entity_type(area_type.clone())
            .scope("/Root/AreaA")
            .build()?,
        Entity::builder()
            .id(area_b_id.clone())
            .entity_type(area_type.clone())
            .scope("/Root/AreaB")
            .build()?,
        Entity::builder().id(area_c_id).entity_type(area_type.clone()).scope("/Root/AreaC").build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/Madrid/Gardens/ParqueNorte,/CompanyA/OrganizationB/UnitC".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&building_2_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&area_type)
        .scope_q("/Root/AreaA,/Root/AreaB".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&area_a_id)));
    expect_that!(page_2.items(), contains(has_id(&area_b_id)));
    // | is the same OR operator as ,.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&area_type)
        .scope_q("/Root/AreaA|/Root/AreaB".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&area_a_id)));
    expect_that!(page_3.items(), contains(has_id(&area_b_id)));
    Ok(())
}
