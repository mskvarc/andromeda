//! The alias under a tenant differs from the default tenant alias.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity Information under NGSILD-Tenant answers the `contextSourceAlias` of that
/// Tenant, which differs from the alias answered without the header, with the same id
/// Clause: 5.2.40, 4.14
/// Tenant: private (the subject is the alias a Tenant of its own is identified by)
#[gtest]
#[tokio::test]
async fn n_inf_ret_6004_the_alias_under_a_tenant_differs_from_the_default_tenant_alias() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    let id_default = identity.id;
    let alias_default = identity.alias;
    let identity_2 = tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_2.id, eq(&id_default));
    expect_that!(&identity_2.alias, not(eq(&alias_default)));
    Ok(())
}
