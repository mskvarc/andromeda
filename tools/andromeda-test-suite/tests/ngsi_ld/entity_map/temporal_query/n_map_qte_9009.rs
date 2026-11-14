//! A temporal entity map creation takes a suggested lifetime.

use andromeda_test_suite::{broker, header_matcher::present, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities through GET `/temporal/entityMaps` with a type,
/// a timerel and `timeAt`, and `entityMapLifetime` PT5M and no `entityMap` answers 201 with an `EntityMap` whose
/// `expiresAt` lies more than four and at most five minutes ahead
/// Clause: 5.14.5.3, 5.14.5.4, 6.35.3.1
/// Recorded: clause 5.14.5.3 lists the suggested expiry among the operation's inputs and ignores the `entityMap`
/// flag, the `EntityMap` always being returned, so the Table 6.18.3.2-1 condition on `entityMap` holds
#[gtest]
#[tokio::test]
async fn n_map_qte_9009_a_temporal_entity_map_creation_takes_a_suggested_lifetime() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let plus_240: DateTime = (Utc::now() + TimeDelta::seconds(240)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let plus_301: DateTime = (Utc::now() + TimeDelta::seconds(301)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .lifetime("PT5M".parse()?)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(created.entity_map(), present());
    let map = created.body();
    expect_that!(map.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(map.expires_at.timestamp(), gt(plus_240.timestamp()));
    expect_that!(map.expires_at.timestamp(), lt(plus_301.timestamp()));
    Ok(())
}
