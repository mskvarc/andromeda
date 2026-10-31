//! A listing of the stored `@context`s names the one context the case added, with no `details` or `details=false`, of
//! every kind or of kind `Hosted` (clause 5.13.3).

use crate::jsonld_context::stored_contexts::{add_context, attribute_terms};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::{
    headers::Accept,
    operation::{ContextKindFilter, SummaryDetails},
};

/// Adds the `A1`..`A3` context in a tenant of the case's own, lists the contexts of `kind` with `details`, and
/// requires 200 with a JSON array that names a URL ending in the added context's `Location`.
async fn lists_the_added_context(kind: Option<ContextKindFilter>, details: SummaryDetails) -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let first = add_context(&tenant, &attribute_terms(&namespace, &["A1", "A2", "A3"])?).await?;
    let listed = tenant
        .with_accept(Accept::any())
        .with_responses()
        .contexts()
        .list()
        .maybe_kind(kind)
        .details(details)
        .send()
        .await;
    expect_that!(&listed, answered(StatusCode::OK));
    let listed = listed?;
    expect_that!(wire::json_body(&listed)?.as_array(), some(anything()));
    expect_that!(listed.outcome, contains(displays_as(ends_with(first.location.as_str()))));
    tenant.contexts().delete(&first.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Expected: a listed URL is compared as the broker lists it: clause 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the test looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest, so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_02_01_list_contexts_no_details_no_kind_one_added() -> Result<()> {
    lists_the_added_context(None, SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Expected: a listed URL is compared as the broker lists it: clause 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the test looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest, so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_02_02_list_contexts_no_details_kind_hosted_one_added() -> Result<()> {
    lists_the_added_context(Some(ContextKindFilter::Hosted), SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Expected: a listed URL is compared as the broker lists it: clause 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the test looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest, so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_02_03_list_contexts_details_false_no_kind_one_added() -> Result<()> {
    lists_the_added_context(None, SummaryDetails::Explicit).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Expected: a listed URL is compared as the broker lists it: clause 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the test looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest, so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_02_04_list_contexts_details_false_kind_hosted_one_added() -> Result<()> {
    lists_the_added_context(Some(ContextKindFilter::Hosted), SummaryDetails::Explicit).await
}
