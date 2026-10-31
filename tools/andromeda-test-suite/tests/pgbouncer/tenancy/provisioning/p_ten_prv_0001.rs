//! First writes into new tenants behind a transaction pooler each create their tenant.

use andromeda_test_suite::{broker, fresh_tenant_writes::expect_first_writes_create_their_tenants, namespace::Namespace};
use googletest::prelude::*;

/// The first writes sent at once, each into a tenant of its own.
const PARALLEL_FIRST_WRITES: usize = 20;

/// Pins: 20 Create Entity requests sent at once through a broker serving behind a transaction pooler, each naming a
/// tenant that does not exist yet, each answer 201, and each tenant then holds exactly the Entity written into it
/// Clause: 5.5.10, 5.6.1, 6.3.2
/// Tenant: private (the subject is the implicit creation of new tenants by their first write)
#[gtest]
#[tokio::test]
async fn p_ten_prv_0001_first_writes_into_new_tenants_behind_a_transaction_pooler_each_create_their_tenant() -> Result<()> {
    expect_first_writes_create_their_tenants(&broker::front()?, &Namespace::fresh(), PARALLEL_FIRST_WRITES).await
}
