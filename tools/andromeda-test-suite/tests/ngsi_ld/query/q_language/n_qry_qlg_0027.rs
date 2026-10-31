//! A sub attribute of any dataset instance satisfies the term.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Number, Property, Value},
    query::q,
};

/// Pins: Query Entities with q on a sub-attribute answers the entity when the instance of another `datasetId` than
/// the default holds a matching value
/// Clause: 4.9, 4.5.5
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0027_a_sub_attribute_of_any_dataset_instance_satisfies_the_term() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("temperature", Property::builder(20).property("accuracy", Value::Number("0.1".parse()?)))
        .property(
            "temperature",
            Property::builder(21)
                .dataset_id(namespace.dataset_id("Dataset", 1)?)
                .property("accuracy", Value::Number("0.9".parse()?)),
        )
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").gt("0.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
