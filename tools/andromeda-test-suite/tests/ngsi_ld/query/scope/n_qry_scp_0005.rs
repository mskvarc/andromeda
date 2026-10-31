//! A parenthesised conjunction selects entities in every scope.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::scope_query::ScopeQuery,
};

/// Pins: A parenthesised scope conjunction selects only entities in every scope it names, and it combines with a
/// disjunction
/// Clause: 4.19, 5.7.2
/// Recorded: clause 4.19 derives a conjunction of scopes only inside parentheses, so the conjunction is sent as
/// (/A;/B); the bare form /A;/B is not part of the grammar
#[gtest]
#[tokio::test]
async fn n_qry_scp_0005_a_parenthesised_conjunction_selects_entities_in_every_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let site_id = namespace.urn("Site", 1)?;
    let site_type = namespace.entity_type("Site")?;
    let site_2_id = namespace.urn("Site", 2)?;
    let site_3_id = namespace.urn("Site", 3)?;
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
            .id(site_id.clone())
            .entity_type(site_type.clone())
            .scope(["/Aa", "/Bb"])
            .build()?,
        Entity::builder().id(site_2_id.clone()).entity_type(site_type.clone()).scope("/Cc").build()?,
        Entity::builder().id(site_3_id).entity_type(site_type.clone()).scope("/Aa").build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Only the Building in both scopes.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("(/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC)".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    // Site 3 is in /Aa but not in /Bb.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&site_type)
        .scope_q("(/Aa;/Bb)|/Cc".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&site_id)));
    expect_that!(page_2.items(), contains(has_id(&site_2_id)));
    Ok(())
}
