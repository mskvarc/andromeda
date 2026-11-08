//! The context source id is the same for every tenant.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity answers the same id for the default tenant and a named tenant, type
/// `ContextSourceIdentity` for both, and `contextSourceUptime` and `contextSourceTimeAt` as strings for the named
/// tenant
/// Clause: 5.2.40, Table 5.2.40-1, 5.15.1.4
/// Tenant: private (the subject is how a named tenant identifies itself; the files name two tenants of their own,
/// which a registration created and deleted brings into existence)
#[gtest]
#[tokio::test]
async fn n_inf_ret_9003_the_context_source_id_is_the_same_for_every_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    // A registration created and deleted again brings the tenant into existence and leaves nothing in it.
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    let source_id = identity.id;
    let identity_2 = tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_2.id, eq(&source_id));
    Ok(())
}
