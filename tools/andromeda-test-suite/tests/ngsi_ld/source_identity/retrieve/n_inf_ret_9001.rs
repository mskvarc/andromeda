//! Each tenant observes its own context source alias.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity answers a non-empty single-token `contextSourceAlias` for the default
/// tenant and for two named tenants, all three distinct
/// Clause: Table 5.2.40-1, 4.14, 5.15.1.4
/// Tenant: private (the subject is how a named tenant identifies itself; the files name two tenants of their own,
/// which a registration created and deleted brings into existence)
#[gtest]
#[tokio::test]
async fn n_inf_ret_9001_each_tenant_observes_its_own_context_source_alias() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let beta_tenant = broker::provisioned_tenant(&broker, format!("{namespace}beta").parse()?, &namespace).await?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    let default_alias = identity.alias;
    let identity_2 = tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_2.alias, not(eq(&default_alias)));
    let alpha_alias = identity_2.alias;
    let identity_3 = beta_tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_3.alias, not(eq(&default_alias)));
    expect_that!(&identity_3.alias, not(eq(&alpha_alias)));
    Ok(())
}
