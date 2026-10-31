//! Q reads the latest recorded instance inside the window and in effect when it opens.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::{q, temporal_query},
};

/// Pins: of three Vehicles whose `brandName` and speed Core writes set, with an after window opening at 12:30, q
/// brandName=="Volvo" selects the one whose only `brandName`, Volvo, observed at 12:00 is in effect when the window
/// opens, and the one updated to Volvo at 12:50 inside it, but not the one updated from Volvo to Seat at 12:10
/// Clause: 4.3.1, 4.9, 4.11, 5.7.4.4, Annex C clause C.5.5
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9081_q_reads_the_latest_recorded_instance_inside_the_window_and_in_effect_when_it_opens() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_steady_id = namespace.urn("Vehicle", "steady")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_rebranded_id = namespace.urn("Vehicle", "rebranded")?;
    let vehicle_sold_id = namespace.urn("Vehicle", "sold")?;
    let vehicle_steady = Entity::builder()
        .id(vehicle_steady_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("Volvo").observed_at("2020-10-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(0).observed_at("2020-10-01T12:40:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_steady).send().await?.done()?;
    let vehicle_rebranded = Entity::builder()
        .id(vehicle_rebranded_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("Seat").observed_at("2020-10-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(0).observed_at("2020-10-01T12:40:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_rebranded).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("brandName", Property::builder("Volvo").observed_at("2020-10-01T12:50:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_rebranded_id, &fragment).send().await?.done()?;
    let vehicle_sold = Entity::builder()
        .id(vehicle_sold_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("Volvo").observed_at("2020-10-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(0).observed_at("2020-10-01T12:40:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_sold).send().await?.done()?;
    let fragment_2 = EntityFragment::builder()
        .property("brandName", Property::builder("Seat").observed_at("2020-10-01T12:10:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_sold_id, &fragment_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-10-01T12:30:00Z".parse()?))
        .entity_type(&vehicle_type)
        .q(q::attr("brandName").eq("Volvo"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&vehicle_steady_id)));
    expect_that!(page.items(), contains(has_id(&vehicle_rebranded_id)));
    Ok(())
}
