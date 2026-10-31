//! A post query no registration matches is ordered without local.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType},
    query::ordering::OrderBy,
};

/// Pins: Query Entities by POST with ordering `orderBy` name;desc and no local member, over a type no registration
/// covers, answers 200 with the Entities descending by name
/// Clause: 4.23.1, 5.5.13, 5.7.2.4, 6.23.3.1
/// Recorded: 5.7.2.4 and 5.7.4.4 refuse an ordering whose execution is not limited to the local scope (5.5.13),
/// which 5.5.13 defines as using no Context Source Registration; whether a query no registration matches is in that
/// state without local=true is left open. Andromeda admits the ordering when no registration takes part in the
/// query
#[gtest]
#[tokio::test]
async fn o_qry_ord_9025_a_post_query_no_registration_matches_is_ordered_without_local() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let scope_type = namespace.entity_type("OrderScope")?;
    let mut ids = Vec::new();
    for n in 1..=3 {
        ids.push(namespace.urn("Building", format!("order-{n}"))?);
    }
    for (id, name) in ids.iter().zip(["A", "B", "C"]) {
        let building = Entity::builder()
            .id(id.clone())
            .entity_type(scope_type.clone())
            .property("name", name)
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&scope_type)
        .ordering("name;desc".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), ids_in_order(&[ids[2].clone(), ids[1].clone(), ids[0].clone()]));
    Ok(())
}
