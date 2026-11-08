//! Following next without local returns every entity once in identifier order.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, NgsiLdMediaType},
};

/// Pins: Query Entities with limit 2 over 7 members without local, followed by its next pointers, answers every
/// member once in ascending identifier order, the last page with a prev pointer and no next
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute
/// to it; in the shared tenant a registration of another file naming no entity type matches every type (clause
/// 5.12) and makes the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_qry_pag_0028_following_next_without_local_returns_every_entity_once_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0001_id = namespace.urn("Building", "0001")?;
    let building_0002_id = namespace.urn("Building", "0002")?;
    let building_0003_id = namespace.urn("Building", "0003")?;
    let building_0004_id = namespace.urn("Building", "0004")?;
    let building_0005_id = namespace.urn("Building", "0005")?;
    let building_0006_id = namespace.urn("Building", "0006")?;
    let entities = (0..=6)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:04}"))?)
                .entity_type(building_type.clone())
                .property("name", format!("b{n}"))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for entity in &entities {
        tenant.entities().create(entity).send().await?.done()?;
    }
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&building_0001_id)));
    let page_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&building_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_0003_id)));
    let page_3 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.next_page().or_fail()?)
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&building_0004_id)));
    expect_that!(page_3.items(), nth(1, has_id(&building_0005_id)));
    let page_4 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_3.next_page().or_fail()?)
        .await?;
    expect_that!(page_4.next(), none());
    expect_that!(page_4.prev(), some(anything()));
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&building_0006_id)));
    Ok(())
}
