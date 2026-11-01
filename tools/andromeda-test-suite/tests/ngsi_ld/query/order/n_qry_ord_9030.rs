//! A declared number sorts among the numbers, the infinities and NaN at their XSD places.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, value::xsd_typed};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType, Value},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy=reading` over a JSON number 5 and the xsd:double values "1.5", "-INF", "INF" and "NaN" orders
/// -INF, 1.5, 5, INF, NaN: a declared number is a number, -INF is below every finite number, INF above, and NaN
/// after INF
/// Clause: 4.23.2, 4.9, C.6
#[gtest]
#[tokio::test]
async fn n_qry_ord_9030_a_declared_number_sorts_among_the_numbers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let number_id = namespace.urn("Sensor", "number")?;
    let declared_id = namespace.urn("Sensor", "declared")?;
    let negative_infinity_id = namespace.urn("Sensor", "negative-infinity")?;
    let infinity_id = namespace.urn("Sensor", "infinity")?;
    let not_a_number_id = namespace.urn("Sensor", "not-a-number")?;
    let sensor = |id: &EntityId, reading: Value| {
        Entity::builder()
            .id(id.clone())
            .entity_type(sensor_type.clone())
            .property("reading", reading)
            .build()
    };
    let entities = [
        sensor(&number_id, Value::from(5))?,
        sensor(&declared_id, xsd_typed("double", "1.5")?)?,
        sensor(&negative_infinity_id, xsd_typed("double", "-INF")?)?,
        sensor(&infinity_id, xsd_typed("double", "INF")?)?,
        sensor(&not_a_number_id, xsd_typed("double", "NaN")?)?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .ordering("reading".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(5)));
    expect_that!(page.items(), nth(0, has_id(&negative_infinity_id)));
    expect_that!(page.items(), nth(1, has_id(&declared_id)));
    expect_that!(page.items(), nth(2, has_id(&number_id)));
    expect_that!(page.items(), nth(3, has_id(&infinity_id)));
    expect_that!(page.items(), nth(4, has_id(&not_a_number_id)));
    Ok(())
}
