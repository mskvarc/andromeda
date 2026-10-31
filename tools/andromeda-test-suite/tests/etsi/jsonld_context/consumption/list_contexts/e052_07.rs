//! An entity created in the `@context` of a remote document makes the broker download it, and a `details=true` listing
//! of kind `Cached` describes it under its original URL (clauses 5.13.3 and 5.13.3.5).

use crate::{
    building_payloads::building_simple_attributes,
    jsonld_context::{stored_contexts::expect_context_details, suite_term_document::serve_suite_terms},
};
use andromeda_test_suite::{
    broker,
    eventually::{Poll, delivery_deadline, eventually},
    namespace::Namespace,
    remote_context::RemoteContexts,
    teardown::AnyAnswer,
};
use googletest::prelude::*;
use iri_rs::IriBuf;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, StoredContextKind},
};

/// Pins: Check that one can find a previously cached context
/// Clause: 5.13.3
/// Isolation: the term document is served per file under the mock origin, at `contexts/<ns>/` followed by its host and
/// path, as application/ld+json with every term mapped to its per-file IRI
/// Expected: a Cached @context is listed by exactly the URL it was downloaded from (clause 5.13.3.1: "it shall be the
/// original URL")
/// Expected: lastUsage and numberOfHits are absent or of their type (a `DateTime` and a number, clause 5.13.3.5)
/// Timing: the request that reads a Cached @context right after its download is retried until the delivery deadline (12
/// times 250 ms): Andromeda records a downloaded @context after the request that downloaded it
/// Tenant: private (the test looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest, so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_07_01_list_contexts_details_true_kind_cached_one_downloaded() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let remote = RemoteContexts::start().await?;
    let suite_terms = serve_suite_terms(&remote, &namespace).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    tenant
        .with_context(Context::from(IriBuf::new(suite_terms.as_str())?))?
        .entities()
        .create(&building)
        .send()
        .await?;
    let lister = tenant.with_accept(Accept::any());
    let listing = eventually(delivery_deadline(), || async {
        match lister.contexts().list_details().kind(ContextKindFilter::Cached).send().await {
            Ok(listing) if listing.iter().any(|entry| entry.url.as_str() == suite_terms.as_str()) => Poll::Ready(listing),
            Ok(listing) => Poll::Pending(format!("{listing:?}")),
            Err(error) => Poll::Pending(error.to_string()),
        }
    })
    .await?;
    let entry = listing.iter().find(|entry| entry.url.as_str() == suite_terms.as_str()).or_fail()?;
    expect_context_details(entry, &StoredContextKind::Cached);
    tenant.contexts().delete(&suite_terms.as_str().parse()?).send().await.any_answer();
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
