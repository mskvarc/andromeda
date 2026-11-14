//! A deletion and an observation before the window leave q nothing to read.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, NgsiLdMediaType, Property},
    query::{q, temporal_query},
};

/// Pins: of three Vehicles whose speed is observed in 2099, q `brandName=="Volvo"` in a window opening in 2099 selects
/// only the one whose `brandName` Volvo is observed in 2099, and neither the one whose Volvo observed in 2020 a Core
/// write then deletes nor the one whose Volvo observed in 2020 is kept, since no instance of theirs lies inside the
/// window
/// Clause: 4.3.1, 4.5.7, 4.9, 4.11, 5.7.4.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, and records an attribute deletion at the instant it
/// was deleted
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9377_a_deletion_and_an_observation_before_the_window_leave_q_nothing_to_read() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let scrapped_id = namespace.urn("Vehicle", "scrapped")?;
    let kept_id = namespace.urn("Vehicle", "kept")?;
    let renewed_id = namespace.urn("Vehicle", "renewed")?;
    for id in [&scrapped_id, &kept_id] {
        let vehicle = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brandName", Property::builder("Volvo").observed_at("2020-10-01T12:00:00Z".parse()?))
            .property("speed", Property::builder(0).observed_at("2099-06-01T00:00:00Z".parse()?))
            .build()?;
        broker.entities().create(&vehicle).send().await?.done()?;
    }
    let renewed = Entity::builder()
        .id(renewed_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("Volvo").observed_at("2099-03-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(0).observed_at("2099-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&renewed).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&scrapped_id, &"brandName".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2099-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .q(q::attr("brandName").eq("Volvo"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&renewed_id)));
    Ok(())
}
