//! Concurrent writes to one Entity serialise on two server connections.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    load_outcome::LoadOutcome,
    namespace::Namespace,
    tenant_set::TenantSet,
};
use futures::future::join_all;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityFragment, NgsiLdMediaType},
};

/// The writes sent at once.
const WRITES: i32 = 32;

/// The value a last write sets once the concurrent ones were answered.
const SETTLED: i32 = 1000;

/// Pins: 32 Update Attributes of one Entity's `reading`, each to a distinct value and sent at the same moment through
/// a broker behind a transaction pooler holding two server connections for the tenant's database, are each answered
/// with a success, and a last Update Attributes to 1000 then succeeds and reads back
/// Clause: 5.6.3, 5.7.1
/// Tenant: private (the subject is the server connections of a whole tenant database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// with two server connections per database
#[gtest]
#[tokio::test]
async fn p_sem_wrt_0001_concurrent_writes_to_one_entity_serialise_on_two_server_connections() -> Result<()> {
    let namespace = Namespace::fresh();
    let pooled = broker::client(BrokerRole::PooledDurable)?;
    let set = TenantSet::provision(&pooled, &namespace, 1, 1).await?;
    let tenant = &set.member(0).client;
    let changes = (0..WRITES)
        .map(|value| Ok(EntityFragment::builder().property("reading", value).build()?))
        .collect::<Result<Vec<_>>>()?;
    let written = join_all(
        changes
            .iter()
            .map(|change| async { LoadOutcome::of(&tenant.with_responses().entities().update_attributes(set.device(), change).send().await) }),
    )
    .await;
    expect_that!(written, each(predicate(|answer: &LoadOutcome| answer.is_success())));
    let settled = EntityFragment::builder().property("reading", SETTLED).build()?;
    let settling = LoadOutcome::of(&tenant.with_responses().entities().update_attributes(set.device(), &settled).send().await);
    expect_that!(settling.is_success(), eq(true));
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(set.device())
        .send()
        .await?;
    verify_that!(retrieved.body(), has_attribute("reading", any_attribute().value(SETTLED)))
}
