//! An absent sub attribute satisfies no term.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with q on a sub-attribute the attribute does not carry answers no entity
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0029_an_absent_sub_attribute_satisfies_no_term() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id)
        .entity_type(sensor_type.clone())
        .property("temperature", 20)
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("temperature").sub_attr("accuracy").gt(0))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
