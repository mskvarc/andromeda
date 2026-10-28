//! ETSI `052_03`: a listing of the stored `@context`s names each of the three contexts the case added, with no
//! `details` or `details=false`, of every kind or of kind `Hosted` (clause 5.13.3).

use crate::jsonld_context::stored_contexts::add_initial_set;
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::{
    headers::Accept,
    operation::{ContextKindFilter, SummaryDetails},
};

/// Adds the initial set of three contexts in a tenant of the case's own, lists the contexts of `kind` with `details`,
/// and requires 200 with a JSON array that names a URL ending in each added context's `Location`.
async fn lists_the_added_set(kind: Option<ContextKindFilter>, details: SummaryDetails) -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let [first, second, third] = add_initial_set(&tenant, &namespace).await?;
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
    expect_that!(listed.outcome, contains(displays_as(ends_with(second.location.as_str()))));
    expect_that!(listed.outcome, contains(displays_as(ends_with(third.location.as_str()))));
    tenant.contexts().delete(&first.id).send().await.any_answer();
    tenant.contexts().delete(&second.id).send().await.any_answer();
    tenant.contexts().delete(&third.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Changed: a listed URL is compared as the broker lists it, where the keyword percent-decodes it first: clause
/// 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_03_01_list_contexts_no_details_no_kind_several_added() -> Result<()> {
    lists_the_added_set(None, SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Changed: a listed URL is compared as the broker lists it, where the keyword percent-decodes it first: clause
/// 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_03_02_list_contexts_no_details_kind_hosted_several_added() -> Result<()> {
    lists_the_added_set(Some(ContextKindFilter::Hosted), SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Changed: a listed URL is compared as the broker lists it, where the keyword percent-decodes it first: clause
/// 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_03_03_list_contexts_details_false_no_kind_several_added() -> Result<()> {
    lists_the_added_set(None, SummaryDetails::Explicit).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Changed: a listed URL is compared as the broker lists it, where the keyword percent-decodes it first: clause
/// 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_03_04_list_contexts_details_false_kind_hosted_several_added() -> Result<()> {
    lists_the_added_set(Some(ContextKindFilter::Hosted), SummaryDetails::Explicit).await
}
