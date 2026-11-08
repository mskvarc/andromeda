//! An expiry on a resting tenant is swept within the cadence.

use andromeda_test_suite::{broker, clock_wait::wait_past, eventually::retried, namespace::Namespace, outcome::Done, problem::answered};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType},
};

/// Pins: In a tenant idle for four seconds, an Entity then created with an `expiresAt` two seconds ahead is gone
/// within its `expiresAt` and three sweeps
/// Clause: 4.22, 5.6.1
/// Tenant: private (the subject is a tenant with no outstanding obligation, so its background work is resting when
/// the obligation is created; the shared tenant always has some)
/// Recorded: Andromeda configuration, not v1.9.1: the stack sweeps every second
/// (`temporal.expiration_check_interval`), and a resting tenant waits up to sixteen sweeps; the bound is the
/// obligation's own time plus three sweeps
#[gtest]
#[tokio::test]
async fn a_ent_del_9001_an_expiry_on_a_resting_tenant_is_swept_within_the_cadence() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vehicle_warm_id = namespace.urn("Vehicle", "warm")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_expiring_id = namespace.urn("Vehicle", "expiring")?;
    // A registration created and deleted again brings the tenant into existence and leaves nothing in it.
    let vehicle_warm = Entity::builder().id(vehicle_warm_id).entity_type(vehicle_type.clone()).build()?;
    tenant.entities().create(&vehicle_warm).send().await?.done()?;
    let slept_until = (Utc::now() + TimeDelta::seconds(4)).timestamp();
    wait_past(slept_until).await;
    let in_2_seconds: DateTime = (Utc::now() + TimeDelta::seconds(2)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let vehicle_expiring = Entity::builder()
        .id(vehicle_expiring_id.clone())
        .entity_type(vehicle_type.clone())
        .expires_at(in_2_seconds)
        .build()?;
    tenant.entities().create(&vehicle_expiring).send().await?.done()?;
    // Twenty tries 250 ms apart bound the wait at the expiry plus three sweeps.
    retried(async || {
        let answer = tenant
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_expiring_id)
            .send()
            .await;
        verify_that!(&answer, answered(StatusCode::NOT_FOUND))?;
        Ok(())
    })
    .await?;
    Ok(())
}
