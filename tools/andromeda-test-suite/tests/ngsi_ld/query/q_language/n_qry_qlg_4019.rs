//! Q matches a value held only by a dataset instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: q speed>50 and q speed==99 answer the entity whose default speed instance is 10 and whose `datasetId`
/// instance is 99, and not the entity whose only speed is 10
/// Clause: 4.9, 4.5.5
/// Recorded: clause 4.9 says "If a Property has multiple instances (identified by its respective `datasetId`), and
/// no `datasetId` is explicitly addressed, the target value shall be any Value of such instances"; the q grammar
/// addresses no `datasetId`
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4019_q_matches_a_value_held_only_by_a_dataset_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_two_id = namespace.urn("Vehicle", "two")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_one_id = namespace.urn("Vehicle", "one")?;
    let entities = [
        Entity::builder()
            .id(vehicle_two_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 10)
            .property("speed", Property::builder(99).dataset_id(namespace.dataset_id("Dataset", "gps")?))
            .build()?,
        Entity::builder()
            .id(vehicle_one_id)
            .entity_type(vehicle_type.clone())
            .property("speed", 10)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_two_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").eq(99))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_two_id)));
    Ok(())
}
