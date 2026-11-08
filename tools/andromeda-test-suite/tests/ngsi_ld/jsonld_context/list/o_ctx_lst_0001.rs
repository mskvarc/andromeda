//! List a fresh tenant answers no contexts.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: List @contexts without parameters in a tenant that has stored nothing answers 200 with an empty list
/// Clause: 5.13.3.4, 6.29.3.2
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: clause 5.13.3.4 lists "@contexts currently stored"; the core @context is not a stored entry, so a
/// tenant that has stored nothing lists nothing
#[gtest]
#[tokio::test]
async fn o_ctx_lst_0001_list_a_fresh_tenant_answers_no_contexts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let contexts = tenant.with_accept(Accept::any()).contexts().list().send().await?;
    expect_that!(contexts, len(eq(0)));
    Ok(())
}
