//! First writes into new tenants sent at once each create their tenant.

use andromeda_test_suite::{broker, fresh_tenant_writes::expect_first_writes_create_their_tenants, namespace::Namespace};
use googletest::prelude::*;

/// The first writes sent at once, each into a tenant of its own. The broker keeps 16 tenant runtimes resident, the
/// default tenant's among them, so 15 named tenants fit. The `private-tenants` group runs this test beside three
/// others that hold their tenants idle between requests, and an idle runtime is evicted before one a request holds.
/// Eight writes leave 7 of the 15 for those three, where 12 would leave 3 and a runtime of this burst could be evicted
/// and rebuilt between its write and its check, within the broker's request timeout.
const PARALLEL_FIRST_WRITES: usize = 8;

/// Pins: 8 Create Entity requests sent at once through a broker connecting to PostgreSQL directly, each naming a
/// tenant that does not exist yet, each answer 201, and each tenant then holds exactly the Entity written into it
/// Clause: 5.5.10, 5.6.1, 6.3.2
/// Tenant: private (the subject is the implicit creation of new tenants by their first write)
/// Recorded: clause 5.5.10 makes implicit Tenant creation an implementation option; Andromeda creates a Tenant on
/// its first Create Entity
#[gtest]
#[tokio::test]
async fn o_ten_imp_0005_first_writes_into_new_tenants_sent_at_once_each_create_their_tenant() -> Result<()> {
    expect_first_writes_create_their_tenants(&broker::front()?, &Namespace::fresh(), PARALLEL_FIRST_WRITES).await
}
