//! Retrieve context source identity echoes the tenant header.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdTenant},
    model::NgsiLdMediaType,
};

/// Pins: Retrieve Context Source Identity for a named tenant answers 200 with that tenant in the NGSILD-Tenant
/// header
/// Clause: 6.3.14, 5.15.1.4
/// Tenant: private (the subject is how a named tenant identifies itself; the files name two tenants of their own,
/// which a registration created and deleted brings into existence)
#[gtest]
#[tokio::test]
async fn n_inf_ret_9004_retrieve_context_source_identity_echoes_the_tenant_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    // A registration created and deleted again brings the tenant into existence and leaves nothing in it.
    let headed = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<NgsiLdTenant>().valid().map(NgsiLdTenant::tenant),
        some(eq(&namespace.tenant()?))
    );
    Ok(())
}
