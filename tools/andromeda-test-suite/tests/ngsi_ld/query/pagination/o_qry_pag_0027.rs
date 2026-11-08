//! The default first page offers opaque cursor pointers without local.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities with limit 2 over three members answers a first page whose Link offers next and last
/// pointers carrying a cursor and no offset, and no first or prev
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute
/// to it; in the shared tenant a registration of another file naming no entity type matches every type (clause
/// 5.12) and makes the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor and offers first and last pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_qry_pag_0027_the_default_first_page_offers_opaque_cursor_pointers_without_local() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0001_id = namespace.urn("Building", "0001")?;
    let building_0002_id = namespace.urn("Building", "0002")?;
    let building_0000 = Entity::builder()
        .id(building_0000_id)
        .entity_type(building_type.clone())
        .property("name", "b0")
        .build()?;
    tenant.entities().create(&building_0000).send().await?.done()?;
    let building_0001 = Entity::builder()
        .id(building_0001_id)
        .entity_type(building_type.clone())
        .property("name", "b1")
        .build()?;
    tenant.entities().create(&building_0001).send().await?.done()?;
    let building_0002 = Entity::builder()
        .id(building_0002_id)
        .entity_type(building_type.clone())
        .property("name", "b2")
        .build()?;
    tenant.entities().create(&building_0002).send().await?.done()?;
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.next(), some(anything()));
    expect_that!(page.last(), some(anything()));
    expect_that!(page.first(), none());
    expect_that!(page.prev(), none());
    expect_that!(links(&page), contains(carries_parameter("cursor")));
    expect_that!(links(&page), each(not(carries_parameter("offset"))));
    Ok(())
}
