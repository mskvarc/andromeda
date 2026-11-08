//! A killed pool answers batch upserts 503 with a retry delay, never 500.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    load_outcome::LoadOutcome,
    namespace::Namespace,
    pool_outage::{MAX_OUTAGE_REQUESTS, kill_then_resume, landed_or_paced, through_outage},
    pooler_console::PoolerConsole,
    tenant_management::database_of,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::Entity;
use std::slice;

/// Pins: while the pooler KILLs a tenant's database pool and RESUMEs it a second later, Batch Entity Upsert of one new
/// Entity, sent every 200 ms until one lands after the resume, is answered 201, or 503 with `Retry-After` 1 or 5, or
/// not at all, never 500; and one lands within 45 s
/// Clause: 5.5.6, 6.3.2, 5.6.8
/// Tenant: private (the subject is the pool of a whole tenant database, which the pooler addresses by database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`;
/// the delay a 503 names follows IETF RFC 9110 section 10.2.3
#[gtest]
#[tokio::test]
async fn p_res_out_0002_a_killed_pool_answers_batch_upserts_503_with_a_retry_delay_never_500() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, 1, 1).await?;
    let tenant = set.member(0).client.with_responses();
    let database = database_of(&resilience, &set.member(0).tenant).await?;
    let device_type = namespace.entity_type("Device")?;
    let devices = (0..MAX_OUTAGE_REQUESTS)
        .map(|number| {
            Ok(Entity::builder()
                .id(namespace.urn("Device", format!("outage-{number}"))?)
                .entity_type(device_type.clone())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    let console = PoolerConsole::of(BrokerRole::Resilience)?;
    let drive = through_outage(
        &devices,
        |device| {
            let tenant = &tenant;
            let device = *device;
            async move { LoadOutcome::of(&tenant.entities().batch_upsert(slice::from_ref(device)).send().await) }
        },
        |answer| answer.status() == Some(StatusCode::CREATED),
    );
    let (answered, killed) = tokio::join!(drive, kill_then_resume(&console, &database));
    killed?;
    let answers = answered.into_iter().map(|(_, answer)| answer).collect::<Vec<_>>();
    expect_that!(answers, each(predicate(|answer: &LoadOutcome| landed_or_paced(answer, &[StatusCode::CREATED]))));
    verify_that!(answers.last().and_then(LoadOutcome::status), some(eq(StatusCode::CREATED)))
}
