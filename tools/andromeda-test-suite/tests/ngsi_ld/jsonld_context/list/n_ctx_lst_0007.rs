//! List a fresh tenant details false kind cached is empty.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::http::{
    headers::Accept,
    operation::{ContextKindFilter, SummaryDetails},
};

/// Pins: List @contexts with details=false and kind=Cached in a tenant that has stored nothing answers 200 with an
/// empty list
/// Clause: 5.13.3.4, 6.29.3.2
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
#[gtest]
#[tokio::test]
async fn n_ctx_lst_0007_list_a_fresh_tenant_details_false_kind_cached_is_empty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Cached)
        .details(SummaryDetails::Explicit)
        .send()
        .await?;
    expect_that!(contexts, len(eq(0)));
    Ok(())
}
