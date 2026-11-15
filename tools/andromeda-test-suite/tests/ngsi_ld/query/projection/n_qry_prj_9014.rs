//! An attribute whose only instance expired keeps no Entity under attrs or pick.

use andromeda_test_suite::{broker, clock_wait::wait_past, namespace::Namespace, outcome::Done};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, DateTime, Entity, NgsiLdMediaType, Property},
    query::projection::Projection,
};

/// Pins: Query Entities with attrs=speed, with pick=speed and with omit=id,type,name, over an Entity whose only speed
/// instance carries an expiresAt two seconds ahead, answers no Entity once that instant has passed, and with
/// attrs=name, a Property with no expiry, answers the Entity
/// Clause: 4.22, 5.2.23, 4.21, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_prj_9014_an_expired_attribute_keeps_no_entity_under_attrs_or_pick() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let expiry = Utc::now() + TimeDelta::seconds(2);
    let expires_at: DateTime = expiry.format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).expires_at(expires_at))
        .property("name", "Ambulance")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    wait_past(expiry.timestamp()).await;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let by_attrs = json
        .entities()
        .query_simplified()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .attrs(vec!["speed".parse::<AttributeName>()?])
        .send()
        .await?;
    expect_that!(by_attrs.items(), is_empty());
    let picked = json
        .entities()
        .query_projected_simplified()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .pick("speed".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(picked.items(), is_empty());
    let omitted = json
        .entities()
        .query_projected_simplified()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .omit("id,type,name".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(omitted.items(), is_empty());
    let standing = json
        .entities()
        .query_simplified()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id])
        .attrs(vec!["name".parse::<AttributeName>()?])
        .send()
        .await?;
    expect_that!(standing.items(), len(eq(1)));
    Ok(())
}
