//! A tenant is identified by an alias of its own.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity Information under a tenant answers 200 with a `ContextSourceIdentity`
/// whose non-empty `contextSourceAlias` differs from the alias the default tenant answers
/// Clause: 5.15.1.1, 5.2.40, 4.14
/// Tenant: private (the alias of a tenant is asked of a tenant of its own, and a tenant comes into existence only
/// by a write that targets it)
#[gtest]
#[tokio::test]
async fn n_inf_ret_7000_a_tenant_is_identified_by_an_alias_of_its_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    let default_alias = identity.alias;
    let identity_2 = tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_2.alias, not(eq(&default_alias)));
    Ok(())
}
