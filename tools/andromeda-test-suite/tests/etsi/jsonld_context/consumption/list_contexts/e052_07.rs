//! ETSI `052_07`: an entity created in the `@context` of a remote document makes the broker download it, and a
//! `details=true` listing of kind `Cached` describes it under its original URL (clauses 5.13.3 and 5.13.3.5).

use crate::{
    jsonld_context::{stored_contexts::expect_context_details, suite_term_document::serve_suite_terms},
    suite_payloads::building_simple_attributes,
};
use andromeda_test_suite::{
    broker,
    eventually::{Poll, delivery_deadline, eventually},
    namespace::Namespace,
    remote_context::RemoteContexts,
    teardown::AnyAnswer,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriBuf;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{ContextKindFilter, ListContextDetails},
    },
    model::Context,
};
use serde_json::{Value, json};

/// Pins: Check that one can find a previously cached context
/// Clause: 5.13.3
/// Changed: the suite term document the case names is served per file by a WireMock stub,
/// `{{mock_internal}}/contexts/{{ns}}/` followed by its host and path, as application/ld+json with every suite IRI
/// replaced by its per-file IRI (the standing vocabulary substitution)
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Changed: a Cached @context is required to be listed by exactly the URL it was downloaded from (clause 5.13.3.1: "it
/// shall be the original URL"), where the keyword accepts any URL ending with it
/// Changed: lastUsage and numberOfHits are required to be absent or of their type (a `DateTime` and a number, clause
/// 5.13.3.5), where the keyword also skips a null or empty value
/// Changed: the request that reads a Cached @context right after its download is retried until the delivery deadline (12 times
/// 250 ms), the wait the suite itself gives a download to be recorded in `051_05` and `053_05`: Andromeda records a
/// downloaded @context after the request that downloaded it
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_07_01_list_contexts_details_true_kind_cached_one_downloaded() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let remote = RemoteContexts::start().await?;
    let suite_terms = serve_suite_terms(&remote, &namespace).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    tenant
        .with_context(Context::from(IriBuf::new(suite_terms.to_string())?))?
        .entities()
        .create(&building)
        .send()
        .await?;
    // Sent unchecked: the typed listing refuses the `null` `expiresAt` and `extraInfo` the broker writes.
    let lister = tenant.with_accept(Accept::any());
    let listing_request = ListContextDetails::builder().kind(ContextKindFilter::Cached).build();
    let answer = eventually(delivery_deadline(), || async {
        let answer = match lister.unchecked(&listing_request) {
            Ok(request) => lister.send_unchecked(request).await,
            Err(error) => return Poll::Pending(error.to_string()),
        };
        match answer {
            Ok(response) => {
                let listed = serde_json::from_slice::<Value>(response.body()).ok().and_then(|listing| {
                    listing
                        .as_array()
                        .map(|entries| entries.iter().any(|entry| entry.get("URL") == Some(&json!(suite_terms.as_str()))))
                });
                if response.status() == StatusCode::OK && listed == Some(true) {
                    Poll::Ready(response)
                } else {
                    Poll::Pending(format!("{} {:?}", response.status(), response.body()))
                }
            }
            Err(error) => Poll::Pending(error.to_string()),
        }
    })
    .await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let listing = serde_json::from_slice::<Value>(answer.body())?;
    let entries = listing.as_array().or_fail()?;
    let entry = entries.iter().find(|entry| entry.get("URL") == Some(&json!(suite_terms.as_str()))).or_fail()?;
    expect_context_details(entry, ContextKindFilter::Cached);
    tenant.contexts().delete(&suite_terms.as_str().parse()?).send().await.any_answer();
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
