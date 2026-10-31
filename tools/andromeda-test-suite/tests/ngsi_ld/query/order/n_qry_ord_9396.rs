//! orderBy on a unitCode member orders by the code's text.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::{Locality, ordering::OrderBy},
};

/// Pins: Query Entities with `orderBy=speed.unitCode`, ascending and descending, and with
/// `orderBy=speed.accuracy.unitCode` answers 200 with the entities ordered by the text of each `unitCode`, and the
/// entity without one last in both directions
/// Clause: 4.5.2, 4.23.1, 4.23.2, 4.23.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_9396_order_by_on_a_unit_code_member_orders_by_the_code_text() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let unit_order_type = namespace.entity_type("UnitOrder")?;
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|suffix| namespace.urn("Vehicle", suffix));
    let (a, b, c, d) = (a?, b?, c?, d?);
    // Identifier order differs from the expected order in both directions.
    for (id, speed, unit_code) in [
        (&a, 10, Some(UnitCode::Kmh)),
        (&b, 20, Some(UnitCode::Mts)),
        (&c, 30, Some(UnitCode::Hm)),
        (&d, 40, None),
    ] {
        let entity = Entity::builder()
            .id(id.clone())
            .entity_type(unit_order_type.clone())
            .property(
                "speed",
                Property::builder(speed)
                    .maybe_unit_code(unit_code)
                    .property("accuracy", Property::builder(1).maybe_unit_code(unit_code)),
            )
            .build()?;
        broker.entities().create(&entity).send().await?.done()?;
    }
    // Clause 4.23.2: "Attribute does not exist" ranks last, also when the term runs descending.
    for (order_by, expected) in [
        ("speed.unitCode", [&c, &a, &b, &d]),
        ("speed.unitCode;desc", [&b, &a, &c, &d]),
        ("speed.accuracy.unitCode", [&c, &a, &b, &d]),
    ] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&unit_order_type)
            .locality(Locality::Local)
            .ordering(order_by.parse::<OrderBy>()?)
            .send()
            .await?;
        expect_that!(page.items(), ids_in_order(&expected.map(Clone::clone)), "orderBy {order_by}");
    }
    Ok(())
}
