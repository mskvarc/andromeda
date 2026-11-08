//! Every write retried after an outage as `Retry-After` says lands once.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    load_outcome::{LoadOutcome, RetryAfter},
    namespace::Namespace,
    pool_outage::{MAX_OUTAGE_REQUESTS, kill_then_resume, landed_or_paced, through_outage},
    pooler_console::PoolerConsole,
    tenant_management::database_of,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::Entity;
use std::{slice, time::Duration};
use tokio::time::sleep;

/// How often a write is retried before it must have landed.
const RETRIES: usize = 10;

/// The delay a write that got no answer is retried after: the longest a 503 of an outage names.
const UNANSWERED_RETRY: Duration = Duration::from_secs(5);

/// Pins: while the pooler KILLs a tenant's database pool and RESUMEs it a second later, Batch Entity Upsert of one new
/// Entity is sent every 200 ms until one lands after the resume; every write not answered 201 is sent again after the
/// delay its `Retry-After` names (5 s when it got no answer), up to ten times, each answer 201, 204, 503 with
/// `Retry-After` 1 or 5, or none, never 500, until it lands with 201 or 204; afterwards every Entity of the drive
/// is retrieved with a success
/// Clause: 5.5.6, 6.3.2, 5.6.8
/// Tenant: private (the subject is the pool of a whole tenant database, which the pooler addresses by database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`;
/// the delay a 503 names follows IETF RFC 9110 section 10.2.3
#[gtest]
#[tokio::test]
async fn p_res_out_0003_every_write_retried_after_an_outage_as_retry_after_says_lands_once() -> Result<()> {
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
    expect_that!(
        answered,
        each(predicate(|(_, answer): &(&Entity<_>, LoadOutcome)| landed_or_paced(
            answer,
            &[StatusCode::CREATED]
        )))
    );
    for (device, first) in &answered {
        let mut answer = *first;
        for _ in 0..RETRIES {
            if answer.status() == Some(StatusCode::CREATED) || answer.status() == Some(StatusCode::NO_CONTENT) {
                break;
            }
            let delay = match answer {
                LoadOutcome::Answered {
                    retry_after: Some(RetryAfter::Delay(delay)),
                    ..
                } => delay,
                LoadOutcome::Answered { .. } | LoadOutcome::Unanswered(_) => UNANSWERED_RETRY,
            };
            sleep(delay).await;
            answer = LoadOutcome::of(&tenant.entities().batch_upsert(slice::from_ref(*device)).send().await);
            expect_that!(landed_or_paced(&answer, &[StatusCode::CREATED, StatusCode::NO_CONTENT]), eq(true));
        }
        expect_that!(
            answer.status(),
            some(any![eq(StatusCode::CREATED), eq(StatusCode::NO_CONTENT)]),
            "{}",
            device.id
        );
    }
    for (device, _) in &answered {
        let read = LoadOutcome::of(&tenant.entities().retrieve(&device.id).send().await);
        expect_that!(read.is_success(), eq(true), "{}", device.id);
    }
    Ok(())
}
