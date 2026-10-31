//! A trailing hash matches the scope and every scope below.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::scope_query::ScopeQuery,
};

/// Pins: A trailing # matches the given scope and every scope below it, and never the scope's parent
/// Clause: 4.19, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_scp_0003_a_trailing_hash_matches_the_scope_and_every_scope_below() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let zone_a_id = namespace.urn("Zone", "a")?;
    let zone_type = namespace.entity_type("Zone")?;
    let zone_b_id = namespace.urn("Zone", "b")?;
    let zone_outside_id = namespace.urn("Zone", "outside")?;
    let garden_madrid_id = namespace.urn("Garden", "madrid")?;
    let garden_type = namespace.entity_type("Garden")?;
    let garden_gardens_id = namespace.urn("Garden", "gardens")?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .scope("/Madrid/Gardens/ParqueNorte")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .scope(["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"])
            .build()?,
        Entity::builder()
            .id(zone_a_id.clone())
            .entity_type(zone_type.clone())
            .scope("/Root/AreaA/Zone1")
            .build()?,
        Entity::builder()
            .id(zone_b_id.clone())
            .entity_type(zone_type.clone())
            .scope("/Root/AreaA/Zone2")
            .build()?,
        Entity::builder()
            .id(zone_outside_id)
            .entity_type(zone_type.clone())
            .scope("/Root/AreaB/Zone1")
            .build()?,
        Entity::builder()
            .id(garden_madrid_id)
            .entity_type(garden_type.clone())
            .scope("/Madrid")
            .build()?,
        Entity::builder()
            .id(garden_gardens_id.clone())
            .entity_type(garden_type.clone())
            .scope("/Madrid/Gardens")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/CompanyA/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    // The sibling branch AreaB is not reached.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&zone_type)
        .scope_q("/Root/AreaA/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&zone_a_id)));
    expect_that!(page_2.items(), contains(has_id(&zone_b_id)));
    // 4.19 EXAMPLE 2: the scope itself matches; its parent /Madrid does not.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&garden_type)
        .scope_q("/Madrid/Gardens/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&garden_gardens_id)));
    Ok(())
}
