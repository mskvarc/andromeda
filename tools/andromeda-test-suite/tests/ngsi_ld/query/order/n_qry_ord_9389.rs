//! A compound member declared as an XSD number sorts among the numbers.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    value::{object, xsd_typed},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType, Value},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy=reading[level]` over a compound `reading` whose `level` is the JSON number 9 and two whose `level`
/// is declared the xsd:integer "8" and "10" orders 8, 9, 10, and `orderBy=reading[level];desc` 10, 9, 8: a member a
/// value object declares an XSD number is a number, and the datatype order compares numbers with numbers
/// Clause: 4.23.1, 4.23.2, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_ord_9389_a_compound_member_declared_as_an_xsd_number_sorts_among_the_numbers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let gauge_type = namespace.entity_type("Gauge")?;
    let plain_id = namespace.urn("Gauge", "plain")?;
    let lower_id = namespace.urn("Gauge", "lower")?;
    let higher_id = namespace.urn("Gauge", "higher")?;
    let gauge = |id: &EntityId, level: Value| {
        Entity::builder()
            .id(id.clone())
            .entity_type(gauge_type.clone())
            .property("reading", object([("level", level)]))
            .build()
    };
    let entities = [
        gauge(&plain_id, Value::from(9))?,
        gauge(&lower_id, xsd_typed("integer", "8")?)?,
        gauge(&higher_id, xsd_typed("integer", "10")?)?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    for (ordering, expected) in [
        ("reading[level]", [&lower_id, &plain_id, &higher_id]),
        ("reading[level];desc", [&higher_id, &plain_id, &lower_id]),
    ] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&gauge_type)
            .locality(Locality::Local)
            .ordering(ordering.parse::<OrderBy>()?)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(3)), "orderBy={ordering}");
        for (position, id) in expected.into_iter().enumerate() {
            expect_that!(page.items(), nth(position, has_id(id)), "orderBy={ordering}");
        }
    }
    Ok(())
}
