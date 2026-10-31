//! Deleting the core `@context`, with no `reload` or `reload=false`, is refused with 404 `ResourceNotFound`, the core
//! `@context` being no stored context (clause 5.13.5).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::ContextDeletion,
    model::{CORE_CONTEXT, ContextId, ErrorType},
};

/// Deletes the core `@context` with `deletion` in a tenant of the case's own, and requires 404 `ResourceNotFound` with
/// a non-empty title and a non-empty detail that differs from it.
async fn refused_core_deletion(deletion: ContextDeletion) -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let core: ContextId = CORE_CONTEXT.as_str().parse()?;
    let deleted = tenant.with_responses().contexts().delete(&core).deletion(deletion).send().await;
    expect_that!(&deleted, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_distinct_detail());
    Ok(())
}

/// Pins: Check that one gets an error when try to delete the core @context
/// Clause: 5.13.5
/// Expected: the core @context URL is no stored identifier, so clause 5.13.5.4 answers `ResourceNotFound` rather than
/// the `BadRequestData` a stored non-Cached entry earns
/// Tenant: private (the case deletes the core @context, which every file relies on; a broker that failed to refuse it
/// would remove or replace it in the shared tenant)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e051_08_01_delete_a_core_contexts_and_return_an_error_with_no_reload() -> Result<()> {
    refused_core_deletion(ContextDeletion::Remove).await
}

/// Pins: Check that one gets an error when try to delete the core @context
/// Clause: 5.13.5
/// Expected: the core @context URL is no stored identifier, so clause 5.13.5.4 answers `ResourceNotFound` rather than
/// the `BadRequestData` a stored non-Cached entry earns
/// Tenant: private (the case deletes the core @context, which every file relies on; a broker that failed to refuse it
/// would remove or replace it in the shared tenant)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e051_08_02_delete_a_core_contexts_and_return_an_error_with_reload_set_to_false() -> Result<()> {
    refused_core_deletion(ContextDeletion::RemoveExplicitly).await
}
