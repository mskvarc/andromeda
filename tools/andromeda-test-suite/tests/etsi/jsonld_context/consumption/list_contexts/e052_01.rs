//! ETSI `052_01`: a listing of the stored `@context`s answers 200 with a JSON array, whatever summary `details` and
//! `kind` it names (clause 5.13.3).

use andromeda_test_suite::{broker, problem::answered, wire};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::{
    headers::Accept,
    operation::{ContextKindFilter, SummaryDetails},
};

/// Lists the stored contexts of `kind` with `details`, and requires 200 with a JSON array.
async fn lists_an_array(kind: Option<ContextKindFilter>, details: SummaryDetails) -> Result<()> {
    let broker = broker::front()?;
    let listed = broker
        .with_accept(Accept::any())
        .with_responses()
        .contexts()
        .list()
        .maybe_kind(kind)
        .details(details)
        .send()
        .await;
    expect_that!(&listed, answered(StatusCode::OK));
    let body = wire::json_body(&listed?)?;
    expect_that!(body.as_array(), some(anything()));
    Ok(())
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_01_01_list_contexts_no_details_no_kind_nothing_added() -> Result<()> {
    lists_an_array(None, SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_01_02_list_contexts_no_details_kind_hosted_nothing_added() -> Result<()> {
    lists_an_array(Some(ContextKindFilter::Hosted), SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_01_03_list_contexts_no_details_kind_cached_nothing_added() -> Result<()> {
    lists_an_array(Some(ContextKindFilter::Cached), SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_01_04_list_contexts_no_details_kind_implicitly_created_nothing_added() -> Result<()> {
    lists_an_array(Some(ContextKindFilter::ImplicitlyCreated), SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_01_05_list_contexts_details_false_no_kind_nothing_added() -> Result<()> {
    lists_an_array(None, SummaryDetails::Explicit).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_01_06_list_contexts_details_false_kind_hosted_nothing_added() -> Result<()> {
    lists_an_array(Some(ContextKindFilter::Hosted), SummaryDetails::Explicit).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_01_07_list_contexts_details_false_kind_cached_nothing_added() -> Result<()> {
    lists_an_array(Some(ContextKindFilter::Cached), SummaryDetails::Explicit).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_01_08_list_contexts_details_false_kind_implicitly_created_nothing_added() -> Result<()> {
    lists_an_array(Some(ContextKindFilter::ImplicitlyCreated), SummaryDetails::Explicit).await
}
