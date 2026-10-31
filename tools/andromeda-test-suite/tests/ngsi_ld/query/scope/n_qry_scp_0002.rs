//! Plus matches exactly one scope level.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::scope_query::ScopeQuery,
};

/// Pins: + matches exactly one scope level of any name, inside a path or at its end
/// Clause: 4.19, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_scp_0002_plus_matches_exactly_one_scope_level() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let plaza_id = namespace.urn("Plaza", 1)?;
    let plaza_type = namespace.entity_type("Plaza")?;
    let plaza_2_id = namespace.urn("Plaza", 2)?;
    let plaza_3_id = namespace.urn("Plaza", 3)?;
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
        Entity::builder().id(plaza_id).entity_type(plaza_type.clone()).scope("/Madrid").build()?,
        Entity::builder()
            .id(plaza_2_id.clone())
            .entity_type(plaza_type.clone())
            .scope("/Madrid/X")
            .build()?,
        Entity::builder().id(plaza_3_id).entity_type(plaza_type.clone()).scope("/Madrid/X/Y").build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/Madrid/+/ParqueNorte".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&building_2_id)));
    // One level below Madrid, not Madrid itself and not two levels below.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&plaza_type)
        .scope_q("/Madrid/+".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&plaza_2_id)));
    Ok(())
}
