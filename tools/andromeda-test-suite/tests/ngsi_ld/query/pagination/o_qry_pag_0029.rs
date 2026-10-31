//! From the last page prev pages back to the start without local.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities with limit 2 over five entities without local offers a last pointer to the final two; prev
/// pointers then lead to the second and third entities and to the first alone, which offers no prev pointer
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute
/// to it; in the shared tenant a registration of another file naming no entity type matches every type (clause
/// 5.12) and makes the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor and offers first and last pointers beside next and prev
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_qry_pag_0029_from_the_last_page_prev_pages_back_to_the_start_without_local() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_type = namespace.entity_type("Building")?;
    let mut ids = Vec::new();
    for n in 0..5 {
        let id = namespace.urn("Building", format!("{n:04}"))?;
        let building = Entity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .property("name", format!("b{n}"))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
        ids.push(id);
    }
    let json_ld = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let first = json_ld.entities().query().entity_type(&building_type).limit(2).send().await?;
    let last = json_ld.send(&first.last_page().or_fail()?).await?;
    expect_that!(last.items(), ids_in_order(&ids[3..]));
    let second = json_ld.send(&last.prev_page().or_fail()?).await?;
    expect_that!(second.items(), ids_in_order(&ids[1..3]));
    let start = json_ld.send(&second.prev_page().or_fail()?).await?;
    expect_that!(start.items(), ids_in_order(&ids[..1]));
    expect_that!(start.prev(), none());
    Ok(())
}
