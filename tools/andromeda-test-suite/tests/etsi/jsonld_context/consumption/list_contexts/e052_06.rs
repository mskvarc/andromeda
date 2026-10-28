//! ETSI `052_06`: after a subscription makes the broker download two remote `@context`s and create an
//! `ImplicitlyCreated` one, a listing lists the downloads as `Cached`, the merged context as `ImplicitlyCreated`, and
//! neither as `Hosted` (clauses 5.13.2.4 and 5.13.3).

use crate::jsonld_context::implicit_context::{ImplicitContext, ImplicitTeardown};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    eventually::{Poll, delivery_deadline, eventually},
    namespace::Namespace,
    problem::answered,
    wire,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    client::Received,
    http::{
        headers::Accept,
        operation::{ContextKindFilter, SummaryDetails},
    },
};
use url::Url;

/// Lists every stored context with `details` in the shared tenant, after the implicit setup: requires 200 only, the
/// keyword checking nothing more when no kind is given.
async fn lists_every_kind(details: SummaryDetails) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let implicit = ImplicitContext::set_up(&broker, &namespace).await?;
    implicit.expect_served_without_hits(&broker).await?;
    let listed = broker
        .with_accept(Accept::any())
        .with_responses()
        .contexts()
        .list()
        .details(details)
        .send()
        .await;
    expect_that!(&listed, answered(StatusCode::OK));
    implicit.delete(&broker, ImplicitTeardown::Keep).await
}

/// Lists the `Hosted` contexts with `details` in a tenant of the case's own, after the implicit setup: requires 200
/// and an empty JSON array.
async fn lists_nothing_hosted(details: SummaryDetails) -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let implicit = ImplicitContext::set_up(&tenant, &namespace).await?;
    implicit.expect_served_without_hits(&tenant).await?;
    let listed = tenant
        .with_accept(Accept::any())
        .with_responses()
        .contexts()
        .list()
        .kind(ContextKindFilter::Hosted)
        .details(details)
        .send()
        .await;
    expect_that!(&listed, answered(StatusCode::OK));
    let listed = listed?;
    expect_that!(wire::json_body(&listed)?.as_array(), some(anything()));
    expect_that!(listed.outcome, is_empty());
    implicit.delete(&tenant, ImplicitTeardown::Keep).await
}

/// Lists the `Cached` contexts with `details` in a tenant of the case's own, after the implicit setup, retried for up
/// to 3 s until both downloads are listed: requires 200 and a JSON array naming each by its original URL.
async fn lists_both_cached(details: SummaryDetails) -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let implicit = ImplicitContext::set_up(&tenant, &namespace).await?;
    implicit.expect_served_without_hits(&tenant).await?;
    let listed = cached_listing(&tenant, &implicit, details).await?;
    expect_that!(listed.status, eq(StatusCode::OK));
    expect_that!(wire::json_body(&listed)?.as_array(), some(anything()));
    expect_that!(listed.outcome, contains(eq(implicit.test_context())));
    expect_that!(listed.outcome, contains(eq(implicit.smart_data_models())));
    implicit.delete(&tenant, ImplicitTeardown::Keep).await
}

/// The `Cached` listing of `tenant`, polled every 250 ms until the delivery deadline until it names both remote documents of
/// `implicit`: the broker records a download after the request that made it.
async fn cached_listing(tenant: &BrokerClient, implicit: &ImplicitContext, details: SummaryDetails) -> Result<Received<Vec<Url>>> {
    let lister = tenant.with_accept(Accept::any()).with_responses();
    Ok(eventually(delivery_deadline(), || async {
        match lister.contexts().list().kind(ContextKindFilter::Cached).details(details).send().await {
            Ok(received) if received.outcome.contains(implicit.test_context()) && received.outcome.contains(implicit.smart_data_models()) => {
                Poll::Ready(received)
            }
            Ok(received) => Poll::Pending(format!("{:?}", received.outcome)),
            Err(error) => Poll::Pending(error.to_string()),
        }
    })
    .await?)
}

/// Lists the `ImplicitlyCreated` contexts with `details` in a tenant of the case's own, after the implicit setup:
/// requires 200 and a JSON array naming a URL that ends with the last segment of the subscription's `jsonldContext`.
async fn lists_the_implicit(details: SummaryDetails) -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let implicit = ImplicitContext::set_up(&tenant, &namespace).await?;
    implicit.expect_served_without_hits(&tenant).await?;
    let listed = tenant
        .with_accept(Accept::any())
        .with_responses()
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .details(details)
        .send()
        .await;
    expect_that!(&listed, answered(StatusCode::OK));
    let listed = listed?;
    expect_that!(wire::json_body(&listed)?.as_array(), some(anything()));
    expect_that!(listed.outcome, contains(displays_as(ends_with(implicit.id.as_str()))));
    implicit.delete(&tenant, ImplicitTeardown::Keep).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a served @context document is the stored @context alone, clauses 5.13.1 and 5.13.4.4), so its
/// check never applies; the port requires the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_06_01_list_contexts_no_details_no_kind_an_implicitly_created_one() -> Result<()> {
    lists_every_kind(SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a served @context document is the stored @context alone, clauses 5.13.1 and 5.13.4.4), so its
/// check never applies; the port requires the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the Hosted listing is required to be empty, where the keyword requires only that it does not list both
/// Cached @contexts: the tenant is the file's own and nothing Hosted was added to it (clause 5.13.3.4), and an empty
/// listing names neither @context in any spelling
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Tenant: private (the case checks that its Cached @contexts are not listed as Hosted, which only a tenant of its own,
/// holding no other Hosted entry, lets the port assert exactly; the shared tenant's listing also stops at 1000 entries
/// (defect D-25))
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_06_02_list_contexts_no_details_kind_hosted_an_implicitly_created_one() -> Result<()> {
    lists_nothing_hosted(SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a served @context document is the stored @context alone, clauses 5.13.1 and 5.13.4.4), so its
/// check never applies; the port requires the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: a Cached @context is required to be listed by exactly the URL it was downloaded from (clause 5.13.3.1: "it
/// shall be the original URL"), where the keyword accepts any URL ending with it
/// Changed: the request that reads a Cached @context right after its download is retried until the delivery deadline (12 times
/// 250 ms), the wait the suite itself gives a download to be recorded in `051_05` and `053_05`: Andromeda records a
/// downloaded @context after the request that downloaded it
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_06_03_list_contexts_no_details_kind_cached_an_implicitly_created_one() -> Result<()> {
    lists_both_cached(SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a served @context document is the stored @context alone, clauses 5.13.1 and 5.13.4.4), so its
/// check never applies; the port requires the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: a listed URL is compared as the broker lists it, where the keyword percent-decodes it first: clause
/// 5.13.3.1 has a client use each listed URL as given
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_06_04_list_contexts_no_details_kind_implicitly_created_an_implicitly_created_one() -> Result<()> {
    lists_the_implicit(SummaryDetails::Implied).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a served @context document is the stored @context alone, clauses 5.13.1 and 5.13.4.4), so its
/// check never applies; the port requires the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_06_05_list_contexts_details_false_no_kind_an_implicitly_created_one() -> Result<()> {
    lists_every_kind(SummaryDetails::Explicit).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a served @context document is the stored @context alone, clauses 5.13.1 and 5.13.4.4), so its
/// check never applies; the port requires the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the Hosted listing is required to be empty, where the keyword requires only that it does not list both
/// Cached @contexts: the tenant is the file's own and nothing Hosted was added to it (clause 5.13.3.4), and an empty
/// listing names neither @context in any spelling
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Tenant: private (the case checks that its Cached @contexts are not listed as Hosted, which only a tenant of its own,
/// holding no other Hosted entry, lets the port assert exactly; the shared tenant's listing also stops at 1000 entries
/// (defect D-25))
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_06_06_list_contexts_details_false_kind_hosted_an_implicitly_created_one() -> Result<()> {
    lists_nothing_hosted(SummaryDetails::Explicit).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a served @context document is the stored @context alone, clauses 5.13.1 and 5.13.4.4), so its
/// check never applies; the port requires the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: a Cached @context is required to be listed by exactly the URL it was downloaded from (clause 5.13.3.1: "it
/// shall be the original URL"), where the keyword accepts any URL ending with it
/// Changed: the request that reads a Cached @context right after its download is retried until the delivery deadline (12 times
/// 250 ms), the wait the suite itself gives a download to be recorded in `051_05` and `053_05`: Andromeda records a
/// downloaded @context after the request that downloaded it
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_06_07_list_contexts_details_false_kind_cached_an_implicitly_created_one() -> Result<()> {
    lists_both_cached(SummaryDetails::Explicit).await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: the remote @contexts the case names are served per file by WireMock stubs,
/// `{{mock_internal}}/contexts/{{ns}}/`
/// followed by their host and path, as application/ld+json documents mapping the terms the case uses to per-file
/// IRIs
/// Changed: the subscription id gets {{ns}} and its notification endpoint `http://a.b.c/123` becomes
/// `{{mock_internal}}/notify/{{ns}}/1`, as the standing changes give; the case waits for no notification, and none is
/// sent, no entity of its type T1 being written
/// Changed: Check Context Response Body Containing numberOfHits value is handed a body that has no numberOfHits under
/// the specification (a served @context document is the stored @context alone, clauses 5.13.1 and 5.13.4.4), so its
/// check never applies; the port requires the member to be absent, which implies the check
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: a listed URL is compared as the broker lists it, where the keyword percent-decodes it first: clause
/// 5.13.3.1 has a client use each listed URL as given
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_06_08_list_contexts_details_false_kind_implicitly_created_an_implicitly_created_one() -> Result<()> {
    lists_the_implicit(SummaryDetails::Explicit).await
}
