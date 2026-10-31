//! A top level term is satisfied by any dataset instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Query Entities with q comparing an attribute answers the entity when its `datasetId` instance holds a
/// matching value and its default instance does not
/// Clause: 4.9, 4.5.5
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0030_a_top_level_term_is_satisfied_by_any_dataset_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("speed", 10)
        .property("speed", Property::builder(50).dataset_id(namespace.dataset_id("Dataset", 1)?))
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sensor_type)
        .q(q::attr("speed").gt(40))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
