//! A deferred creation is notified once by a broker delivering durably through the pooler.

use andromeda_test_suite::{broker, config::BrokerRole, deferred_creation::expect_deferred_creation_notified_once, namespace::Namespace};
use googletest::prelude::*;

/// Pins: a Batch Entity Create of one Device with `Prefer: respond-async`, in a tenant with a Subscription to the
/// creation of its Devices, through a broker delivering durably behind a transaction pooler, is answered 201 with
/// the preference applied, and the Subscription is notified exactly once, of that Device alone
/// Clause: 5.8.6, 5.6.7
/// Tenant: private (the subject is the side-effect queue of a whole tenant database)
/// Recorded: Andromeda extension, not v1.9.1: `Prefer: respond-async` on a batch operation (IETF RFC 7240 section
/// 4.1) defers its side effects; the broker delivers durably and serves its requests through a transaction-pooling
/// `PgBouncer`
#[gtest]
#[tokio::test]
async fn p_sem_ntf_0002_a_deferred_creation_is_notified_once_by_a_broker_delivering_durably_through_the_pooler() -> Result<()> {
    expect_deferred_creation_notified_once(&broker::client(BrokerRole::PooledDurable)?, &Namespace::fresh()).await
}
