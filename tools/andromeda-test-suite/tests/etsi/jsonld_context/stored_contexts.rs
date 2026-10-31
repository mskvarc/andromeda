//! The `@context`s the `@context` tests store by hand, and the reading of what the broker answers for them.
//!
//! The tests post their own term objects (`A1`, `A2`, ... mapped to `urn:ngsi-ld:attributes:<ns>-A1`), where the other
//! trees store a test vocabulary: the stored document itself is the subject of the test.

use andromeda_test_suite::{
    broker::BrokerClient,
    config::SuiteConfig,
    eventually::{Poll, delivery_deadline, eventually},
    namespace::Namespace,
    teardown::AnyAnswer,
    wire::header_value,
};
use googletest::prelude::*;
use http::header::CONTENT_TYPE;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, ContextId, ContextMetadata, InlineContext, StoredContextKind},
};

/// A context the broker stored: the identifier it gave it and the `Location` it named.
#[derive(Debug)]
pub(crate) struct StoredContext {
    /// The identifier read from the last segment of the `Location`.
    pub(crate) id: ContextId,
    /// The `Location` header of the 201, as the broker wrote it.
    pub(crate) location: IriRefBuf,
}

/// The term object `{"@context": {"<term>": "urn:ngsi-ld:attributes:<ns>-<term>", ...}}` of `terms`, in order.
pub(crate) fn attribute_terms(namespace: &Namespace, terms: &[&str]) -> Result<Context> {
    Ok(Context::Single(ContextEntry::Inline(term_object(namespace, terms).parse::<InlineContext>()?)))
}

/// The document `{"@context": {...}}` of [`attribute_terms`], as the text a remote server serves.
pub(crate) fn attribute_terms_document(namespace: &Namespace, terms: &[&str]) -> String {
    format!("{{\"@context\": {}}}", term_object(namespace, terms))
}

/// The term object mapping each of `terms` to `urn:ngsi-ld:attributes:<ns>-<term>`, in order.
fn term_object(namespace: &Namespace, terms: &[&str]) -> String {
    let members = terms
        .iter()
        .map(|term| format!("\"{term}\": \"urn:ngsi-ld:attributes:{}-{term}\"", namespace.token()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{members}}}")
}

/// Adds `context` with `client` and requires 201 with a `Location` naming the stored context.
pub(crate) async fn add_context(client: &BrokerClient, context: &Context) -> Result<StoredContext> {
    let created = client.contexts().add(context).send().await?;
    let id = created.id().or_fail()?;
    let location = created.location.into_valid().or_fail()?;
    Ok(StoredContext { id, location })
}

/// The Add @context cases: adds `context`, requires 201 with an empty body and a `Location`, then serves the details of
/// the stored context, whatever the status, and requires its `kind` to be `Hosted`.
pub(crate) async fn add_hosted_context(client: &BrokerClient, context: &Context) -> Result<StoredContext> {
    let created = client.with_responses().contexts().add(context).send().await?;
    expect_that!(created.body, is_empty());
    let id = created.outcome.id().or_fail()?;
    let location = created.outcome.location.into_valid().or_fail()?;
    let served = client.with_accept(Accept::any()).contexts().serve_details(&id).send().await?;
    expect_that!(served.kind, eq(&StoredContextKind::Hosted));
    Ok(StoredContext { id, location })
}

/// `Delete Initial @context`, restricted to the case's own contexts: lists the details of every context, whatever the
/// answer, and deletes each of `own`, whatever the broker answers.
pub(crate) async fn delete_own_contexts(client: &BrokerClient, own: &[ContextId]) -> Result<()> {
    client.contexts().list_details().send().await.any_answer();
    for id in own {
        client.contexts().delete(id).send().await.any_answer();
    }
    Ok(())
}

/// Adds the `A1`..`A3`, `B1`..`B3` and `C1`..`C3` contexts, in that order, each
/// required to answer 201 with a `Location`.
pub(crate) async fn add_initial_set(client: &BrokerClient, namespace: &Namespace) -> Result<[StoredContext; 3]> {
    let first = add_context(client, &attribute_terms(namespace, &["A1", "A2", "A3"])?).await?;
    let second = add_context(client, &attribute_terms(namespace, &["B1", "B2", "B3"])?).await?;
    let third = add_context(client, &attribute_terms(namespace, &["C1", "C2", "C3"])?).await?;
    Ok([first, second, third])
}

/// List @contexts with `details=true`, of `kind` when given, with `Accept: */*`: requires 200 and answers the details.
pub(crate) async fn detailed_listing(client: &BrokerClient, kind: Option<ContextKindFilter>) -> Result<Vec<ContextMetadata>> {
    Ok(client.with_accept(Accept::any()).contexts().list_details().maybe_kind(kind).send().await?)
}

/// The element of the `details=true` listing `listing` whose `URL` ends with `location`.
pub(crate) fn listed_entry<'a>(listing: &'a [ContextMetadata], location: &IriRefBuf) -> Option<&'a ContextMetadata> {
    listing.iter().find(|entry| entry.url.as_str().ends_with(location.as_str()))
}

/// The one element of the `details=true` listing `listing` whose `URL` is not the core `@context`, which the broker
/// lists among the `ImplicitlyCreated` entries of every tenant; fails unless there is exactly one.
pub(crate) fn sole_entry_besides_core(listing: &[ContextMetadata]) -> Result<&ContextMetadata> {
    let config = SuiteConfig::get()?;
    let own = listing
        .iter()
        .filter(|entry| entry.url != *config.core_context())
        .collect::<Vec<&ContextMetadata>>();
    verify_that!(own, len(eq(1)))?;
    own.into_iter().next().or_fail()
}

/// The identifier of the listed context `entry`: the last path segment of its `URL`.
pub(crate) fn listed_id(entry: &ContextMetadata) -> Result<ContextId> {
    Ok(entry.url.path_segments().and_then(|mut segments| segments.next_back()).or_fail()?.parse()?)
}

/// Requires the context details `entry` to be of `kind`; the typed read has held the other members to clause 5.13.3.5.
pub(crate) fn expect_context_details(entry: &ContextMetadata, kind: &StoredContextKind) {
    expect_that!(entry.kind, eq(kind));
}

/// Serve @context with `details=true` of `id`, with `Accept: */*`: requires 200 in `application/json` and answers the
/// details.
pub(crate) async fn served_details(client: &BrokerClient, id: &ContextId) -> Result<ContextMetadata> {
    let served = client.with_accept(Accept::any()).with_responses().contexts().serve_details(id).send().await?;
    expect_that!(header_value(served.head.headers(), &CONTENT_TYPE), some(eq("application/json")));
    Ok(served.outcome)
}

/// Serve @context with `details=true` of `id` as [`served_details`] does, retried every 250 ms until the delivery deadline until it
/// answers 200 with details that `holds` accepts: the broker records a downloaded context after the request that
/// downloaded it. Answers the details.
pub(crate) async fn eventual_details(client: &BrokerClient, id: &ContextId, holds: impl Fn(&ContextMetadata) -> bool + Sync) -> Result<ContextMetadata> {
    let client = client.with_accept(Accept::any());
    Ok(eventually(delivery_deadline(), || async {
        match client.contexts().serve_details(id).send().await {
            Ok(details) if holds(&details) => Poll::Ready(details),
            Ok(details) => Poll::Pending(format!("{details:?}")),
            Err(error) => Poll::Pending(error.to_string()),
        }
    })
    .await?)
}
