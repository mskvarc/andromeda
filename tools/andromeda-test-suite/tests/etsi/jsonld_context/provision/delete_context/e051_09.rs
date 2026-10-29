//! ETSI `051_09`: the core `@context` is no stored context: serving its details and reloading it are both answered
//! with 404 (clauses 5.13.4 and 5.13.5).

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{ContextDeletion, ServeContextDetails},
    model::{CORE_CONTEXT, ContextId},
};

/// Pins: Delete a core @contexts with reload set to true and check downloaded core context
/// Clause: 5.13.5
/// Corrected: `fix(051_09_01)`: reload of the non-cached core @context is refused (5.13.5.4)
/// Changed: the core @context URL is no stored identifier, so 5.13.4.4 and 5.13.5.4 answer `ResourceNotFound`;
/// resolution of the core @context itself is unaffected, and o-ctx-del-0007 pins that an entity answer still names it
/// Tenant: private (the case deletes the core @context, which every file relies on; a broker that failed to refuse it
/// would remove or replace it in the shared tenant)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e051_09_01_delete_the_core_context_with_reload_set_to_true_and_serve_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let core: ContextId = CORE_CONTEXT.as_str().parse()?;
    let before = tenant.send_unchecked(tenant.unchecked(&ServeContextDetails { id: &core })?).await;
    expect_that!(&before, answered(StatusCode::NOT_FOUND));
    let reloaded = tenant.with_responses().contexts().delete(&core).deletion(ContextDeletion::Reload).send().await;
    expect_that!(&reloaded, answered(StatusCode::NOT_FOUND));
    let after = tenant.send_unchecked(tenant.unchecked(&ServeContextDetails { id: &core })?).await;
    expect_that!(&after, answered(StatusCode::NOT_FOUND));
    Ok(())
}
