//! A sub attribute equality repeated ten times answers the same page and count every time.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType, Property, Value},
    query::q,
};

/// Pins: Query Entities with q equating a sub-attribute two levels down, sent ten times over, answers the same
/// matching entities and the same total every time, for a match, an offset page of it and a value no entity holds
/// Clause: 4.9, 5.7.2, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0040_a_sub_attribute_equality_repeated_ten_times_answers_the_same_page_and_count_every_time() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let mut ids = Vec::new();
    for (n, (value, accuracy, method)) in [(20, "0.5", "calibrated"), (21, "0.7", "estimated"), (22, "0.4", "calibrated")]
        .into_iter()
        .enumerate()
    {
        let id = namespace.urn("Sensor", n + 1)?;
        let sensor = Entity::builder()
            .id(id.clone())
            .entity_type(sensor_type.clone())
            .property(
                "temperature",
                Property::builder(value).property("accuracy", Property::builder(Value::Number(accuracy.parse()?)).property("method", method)),
            )
            .build()?;
        broker.entities().create(&sensor).send().await?.done()?;
        ids.push(id);
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for _ in 0..10 {
        let page = json
            .entities()
            .query()
            .entity_type(&sensor_type)
            .q(q::attr("temperature").sub_attr("accuracy").sub_attr("method").eq("calibrated"))
            .count(ResultsCount::Include)
            .send()
            .await?;
        expect_that!(page.total(), some(eq(2)));
        expect_that!(page.items(), ids_in_order(&[ids[0].clone(), ids[2].clone()]));
        let offset_page = json
            .entities()
            .query()
            .entity_type(&sensor_type)
            .q(q::attr("temperature").sub_attr("accuracy").sub_attr("method").eq("calibrated"))
            .count(ResultsCount::Include)
            .limit(1)
            .offset(1)
            .send()
            .await?;
        expect_that!(offset_page.total(), some(eq(2)));
        expect_that!(offset_page.items(), ids_in_order(&[ids[2].clone()]));
        let none = json
            .entities()
            .query()
            .entity_type(&sensor_type)
            .q(q::attr("temperature").sub_attr("accuracy").sub_attr("method").eq("uncalibrated"))
            .count(ResultsCount::Include)
            .send()
            .await?;
        expect_that!(none.total(), some(eq(0)));
        expect_that!(none.items(), is_empty());
    }
    Ok(())
}
