//! Attrs alone pages the carriers that sort last in identifier order.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{AttributeName, Entity, NgsiLdMediaType},
};

/// Pins: attrs alone (no type) over 300 entities of which only the last 40 in identifier order carry the attribute
/// pages exactly those 40 in identifier order, twenty at a time, with a total of 40, by GET and by POST query
/// Clause: 5.7.2.4, 5.5.9, 6.3.13, 6.4.3.2
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_qry_ent_0018_attrs_alone_pages_the_carriers_that_sort_last_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let trip_type = namespace.entity_type("Trip")?;
    let route_name = format!("routeName_{namespace}");
    let head_sign = format!("headSign_{namespace}");
    let mut entities = Vec::new();
    for i in 1..=300 {
        let builder = Entity::builder().id(namespace.urn("Trip", format!("{i:04}"))?).entity_type(trip_type.clone());
        entities.push(if i <= 260 {
            builder.property(route_name.as_str(), format!("line {i}")).build()?
        } else {
            builder.property(head_sign.as_str(), format!("line {i}")).build()?
        });
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    // attrs is the only selector; the attribute names carry the namespace, so only this file's entities can hold them
    for method in [QueryMethod::Get, QueryMethod::Post] {
        for (offset, first, expected) in [(0, 261, 20), (20, 281, 20), (40, 301, 0)] {
            let page = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .entities()
                .query()
                .method(method)
                .attrs(vec![head_sign.parse::<AttributeName>()?])
                .limit(20)
                .offset(offset)
                .count(ResultsCount::Include)
                .send()
                .await?;
            expect_that!(page.total(), some(eq(40)));
            expect_that!(page.items(), len(eq(expected)));
            let ids = (first..first + expected)
                .map(|i| namespace.urn("Trip", format!("{i:04}")))
                .collect::<Result<Vec<_>, _>>()?;
            expect_that!(page.items(), ids_in_order(&ids));
            for (item, i) in page.items().iter().zip(first..) {
                expect_that!(item, has_attribute(&head_sign, property().value(format!("line {i}"))));
            }
        }
    }
    Ok(())
}
