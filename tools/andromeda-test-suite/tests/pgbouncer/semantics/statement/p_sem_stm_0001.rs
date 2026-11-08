//! Statements evicted from the pooler's cache are prepared again unnoticed.

use andromeda_test_suite::{broker, config::BrokerRole, load_outcome::LoadOutcome, namespace::Namespace, tenant_set::TenantSet};
use googletest::prelude::*;
use ngsi_ld::query::q;

/// The tenants the queries are spread over.
const TENANTS: usize = 8;

/// How often every query of every tenant is sent.
const ROUNDS: usize = 5;

/// The attributes each `q` shape filters on.
const ATTRIBUTES: [&str; 4] = ["seq", "reading", "name", "status"];

/// The page sizes asked for, one shape each.
const LIMITS: u32 = 24;

/// Pins: 36 Query Entities shapes (`q` greater than and not equal on four attributes, each alone and with a page
/// size, and 24 page sizes), more than the 32 statements the pooler keeps prepared per server, sent in five rounds
/// over 8 tenants through a broker behind a transaction pooler, are each answered with a success
/// Clause: 5.7.2, 4.9
/// Tenant: private (the subject is the statements of several tenant databases)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// with `max_prepared_statements` 32
#[gtest]
#[tokio::test]
async fn p_sem_stm_0001_statements_evicted_from_the_poolers_cache_are_prepared_again_unnoticed() -> Result<()> {
    let namespace = Namespace::fresh();
    let pooled = broker::quiet_client(BrokerRole::PooledDurable)?;
    let set = TenantSet::provision(&pooled, &namespace, TENANTS, TENANTS).await?;
    let device_type = namespace.entity_type("Device")?;
    let mut failures = Vec::new();
    for _ in 0..ROUNDS {
        for member in set.members() {
            let tenant = member.client.with_responses();
            let entities = tenant.entities();
            let query = || entities.query().entity_type(&device_type);
            for attribute in ATTRIBUTES {
                let greater = q::attr(attribute).gt(0).build()?;
                let other = q::attr(attribute).ne(1).build()?;
                for answer in [
                    LoadOutcome::of(&query().q(greater.clone()).send().await),
                    LoadOutcome::of(&query().q(greater).limit(5).send().await),
                    LoadOutcome::of(&query().q(other.clone()).send().await),
                    LoadOutcome::of(&query().q(other).limit(5).send().await),
                ] {
                    if !answer.is_success() {
                        failures.push(answer);
                    }
                }
            }
            for limit in 1..=LIMITS {
                let answer = LoadOutcome::of(&query().limit(limit).send().await);
                if !answer.is_success() {
                    failures.push(answer);
                }
            }
        }
    }
    verify_that!(failures, is_empty())
}
