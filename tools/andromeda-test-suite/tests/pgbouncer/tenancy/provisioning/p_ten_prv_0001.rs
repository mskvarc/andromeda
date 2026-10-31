//! First writes into new tenants behind a transaction pooler each create their tenant.

use andromeda_test_suite::{answered_status::AnsweredStatus, broker, identity_matcher::has_id, namespace::Namespace};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Tenant},
};

/// The first writes sent at once, each into a tenant of its own.
const PARALLEL_FIRST_WRITES: usize = 20;

/// Pins: 20 Create Entity requests sent at once through a broker serving behind a transaction pooler, each naming a
/// tenant that does not exist yet, each answer 201, and each tenant then holds exactly the Entity written into it
/// Clause: 5.5.10, 5.6.1, 6.3.2
/// Tenant: private (the subject is the implicit creation of new tenants by their first write)
#[gtest]
#[tokio::test]
async fn p_ten_prv_0001_first_writes_into_new_tenants_behind_a_transaction_pooler_each_create_their_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let pooled = broker::front()?;
    let device_type = namespace.entity_type("Device")?;
    let device_id = namespace.urn("Device", 1)?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("reading", 1)
        .build()?;
    let mut clients = Vec::with_capacity(PARALLEL_FIRST_WRITES);
    for index in 0..PARALLEL_FIRST_WRITES {
        clients.push(pooled.with_tenant(format!("{namespace}-{index}").parse::<Tenant>()?));
    }
    let clients = clients;
    let answers = join_all(
        clients
            .iter()
            .map(|client| async { client.with_responses().entities().create(&device).send().await }),
    )
    .await;
    expect_that!(
        answers.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
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
