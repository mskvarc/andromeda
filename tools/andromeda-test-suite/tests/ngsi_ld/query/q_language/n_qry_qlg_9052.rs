//! A q filter does not match an expired instance.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::{Locality, q},
};

/// Pins: Of two Vehicles with speed 80, one expiring at 2000-01-01T00:00:00Z and one at 2099-01-01T00:00:00Z, q
/// speed>50 selects only the standing one
/// Clause: 4.22, 4.9, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9052_a_q_filter_does_not_match_an_expired_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_expired_speed_id = namespace.urn("Vehicle", "expired-speed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_standing_speed_id = namespace.urn("Vehicle", "standing-speed")?;
    let vehicle_expired_speed = Entity::builder()
        .id(vehicle_expired_speed_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(80).expires_at("2000-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_expired_speed).send().await?.done()?;
    let vehicle_standing_speed = Entity::builder()
        .id(vehicle_standing_speed_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(80).expires_at("2099-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_standing_speed).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .locality(Locality::Local)
        .ids(vec![vehicle_expired_speed_id.clone(), vehicle_standing_speed_id.clone()])
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_standing_speed_id)));
    Ok(())
}
