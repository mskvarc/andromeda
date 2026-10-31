//! A tenant alias is the configured alias with the tenant appended.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: Retrieve Context Source Identity answers the configured `contextSourceAlias` for the default tenant and
/// that alias followed by a tilde and the tenant name for a named tenant
/// Clause: Table 5.2.40-1, 4.14, 5.15.1.4
/// Tenant: private (the subject is how a named tenant identifies itself; the files name two tenants of their own,
/// which a registration created and deleted brings into existence)
/// Recorded: Table 5.2.40-1 requires an alias identifying the tenant and fixes no spelling; Andromeda keeps the
/// configured alias for the default tenant, which peers already name, and appends ~ and the tenant name for any
/// other
#[gtest]
#[tokio::test]
async fn a_inf_ret_9002_a_tenant_alias_is_the_configured_alias_with_the_tenant_appended() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let private = broker::private_tenant(&front, &namespace).await?;
    let default_alias = front.with_accept(Accept::any()).source_identity().send().await?.alias;
    let named = private.with_accept(Accept::any()).source_identity().send().await?;
    expect_that!(named.alias.to_string(), eq(&format!("{default_alias}~{namespace}")));
    Ok(())
}
