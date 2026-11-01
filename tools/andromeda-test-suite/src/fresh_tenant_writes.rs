//! First writes sent at the same moment, each into a Tenant that does not exist yet, and the Tenants they create
//! (clause 5.5.10): one scenario a broker answers the same whether it reaches PostgreSQL directly or through a
//! transaction pooler, so each stack runs it with its own broker.
//!
//! A burst of first writes saturates the admission of the tenants it creates, so the broker answers 503 naming a
//! delay in `Retry-After` until each tenant is built; a first write is sent again until the delivery deadline, and
//! only the answer that is not a 503 is checked.

use crate::{
    broker::BrokerClient,
    eventually::{POLL_INTERVAL, delivery_deadline},
    identity_matcher::has_id,
    load_outcome::LoadOutcome,
    namespace::Namespace,
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, NgsiLdMediaType, Tenant},
};
use std::time::Instant;
use tokio::time::sleep;

/// Sends `entity` as one Create Entity to `client`, again while the broker answers 503, until the delivery deadline.
pub async fn create_until_admitted(client: &BrokerClient, entity: &Entity<Input>) -> LoadOutcome {
    let deadline = Instant::now() + delivery_deadline();
    loop {
        let outcome = LoadOutcome::of(&client.with_responses().entities().create(entity).send().await);
        if outcome.status() != Some(StatusCode::SERVICE_UNAVAILABLE) || Instant::now() >= deadline {
            return outcome;
        }
        sleep(POLL_INTERVAL).await;
    }
}

/// Sends one Create Entity of the same Device into each of `tenants` Tenants named `<namespace>-<n>`, none of which
/// exists yet, all starting at the same moment through `broker`, and expects each to answer 201 and each Tenant then
/// to hold exactly the Device written into it.
///
/// # Errors
///
/// Fails when a name does not parse, the Device cannot be built or a query of a Tenant is refused.
pub async fn expect_first_writes_create_their_tenants(broker: &BrokerClient, namespace: &Namespace, tenants: usize) -> Result<()> {
    let device_type = namespace.entity_type("Device")?;
    let device_id = namespace.urn("Device", 1)?;
    let device = Entity::builder()
        .id(device_id.clone()) // clone: the listings are compared with the id after the Device is built from it
        .entity_type(device_type.clone()) // clone: the listings select on the type after the Device is built from it
        .property("reading", 1)
        .build()?;
    let clients = (0..tenants)
        .map(|index| Ok(broker.with_tenant(format!("{namespace}-{index}").parse::<Tenant>()?)))
        .collect::<Result<Vec<_>>>()?;
    let answers = join_all(clients.iter().map(|client| create_until_admitted(client, &device))).await;
    expect_that!(
        answers.iter().map(LoadOutcome::status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::CREATED)))
    );
    for client in &clients {
        let listed = client
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&device_type)
            .send()
            .await?;
        expect_that!(listed.items(), elements_are![has_id(&device_id)]);
    }
    Ok(())
}
