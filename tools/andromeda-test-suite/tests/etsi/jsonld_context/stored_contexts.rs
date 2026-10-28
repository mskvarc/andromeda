//! The `@context`s the ETSI `@context` cases store by hand, and the reading of what the broker answers for them.
//!
//! The cases post their own term objects (`A1`, `A2`, ... mapped to `urn:ngsi-ld:attributes:<ns>-A1`), where the
//! other trees store a suite vocabulary: the stored document itself is the subject of the case.

use andromeda_test_suite::{
    broker::BrokerClient,
    config::SuiteConfig,
    eventually::{Poll, delivery_deadline, eventually},
    namespace::Namespace,
    ngsild_date::is_ngsild_date,
    problem::answered,
    teardown::AnyAnswer,
    wire::header_value,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Response, StatusCode, header::CONTENT_TYPE};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{ContextKindFilter, ListContextDetails, ServeContextDetails},
    },
    model::{Context, ContextEntry, ContextId, InlineContext},
};
use serde_json::{Value, json};
use url::Url;

/// The core `@context` the ETSI delete cases name, as their requests write it.
pub(crate) const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

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
    let served = client.send_unchecked(client.unchecked(&ServeContextDetails { id: &id })?).await?;
    expect_that!(serde_json::from_slice::<Value>(served.body())?.get("kind"), some(eq(&json!("Hosted"))));
    Ok(StoredContext { id, location })
}

/// `Delete Initial @context`, restricted to the case's own contexts: lists the details of every context, whatever the
/// answer, and deletes each of `own`, whatever the broker answers.
pub(crate) async fn delete_own_contexts(client: &BrokerClient, own: &[ContextId]) -> Result<()> {
    client.send_unchecked(client.unchecked(&ListContextDetails::default())?).await.any_answer();
    for id in own {
        client.contexts().delete(id).send().await.any_answer();
    }
    Ok(())
}

/// `Create Initial set of @contexts`: adds the `A1`..`A3`, `B1`..`B3` and `C1`..`C3` contexts, in that order, each
/// required to answer 201 with a `Location`.
pub(crate) async fn add_initial_set(client: &BrokerClient, namespace: &Namespace) -> Result<[StoredContext; 3]> {
    let first = add_context(client, &attribute_terms(namespace, &["A1", "A2", "A3"])?).await?;
    let second = add_context(client, &attribute_terms(namespace, &["B1", "B2", "B3"])?).await?;
    let third = add_context(client, &attribute_terms(namespace, &["C1", "C2", "C3"])?).await?;
    Ok([first, second, third])
}

/// List @contexts with `details=true`, of `kind` when given, with `Accept: */*`: requires 200 and answers the body as
/// JSON. The request is sent unchecked because the typed listing refuses the `null` `expiresAt` and `extraInfo` the
/// broker writes, which the case does not read.
pub(crate) async fn detailed_listing(client: &BrokerClient, kind: Option<ContextKindFilter>) -> Result<Value> {
    let client = client.with_accept(Accept::any());
    let request = client.unchecked(&ListContextDetails::builder().maybe_kind(kind).build())?;
    let answer = client.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    Ok(serde_json::from_slice(answer?.body())?)
}

/// The element of the `details=true` listing `listing` whose `URL` ends with `location`, as the keyword searches it.
pub(crate) fn listed_entry<'a>(listing: &'a Value, location: &IriRefBuf) -> Option<&'a Value> {
    listing
        .as_array()?
        .iter()
        .find(|entry| entry.get("URL").and_then(Value::as_str).is_some_and(|url| url.ends_with(location.as_str())))
}

/// The one element of the `details=true` listing `listing` whose `URL` is not the core `@context`, which the broker
/// lists among the `ImplicitlyCreated` entries of every tenant; fails unless there is exactly one, its `URL` a string.
pub(crate) fn sole_entry_besides_core(listing: &Value) -> Result<&Value> {
    let core = json!(SuiteConfig::get()?.core_context().as_str());
    let own = listing
        .as_array()
        .or_fail()?
        .iter()
        .filter(|entry| entry.get("URL") != Some(&core))
        .collect::<Vec<&Value>>();
    verify_that!(own, len(eq(1)))?;
    let entry = own.first().or_fail()?;
    verify_that!(entry.get("URL").and_then(Value::as_str), some(anything()))?;
    Ok(entry)
}

/// The identifier of the listed context `entry`: the last path segment of its `URL`.
pub(crate) fn listed_id(entry: &Value) -> Result<ContextId> {
    let url = Url::parse(entry.get("URL").and_then(Value::as_str).or_fail()?)?;
    Ok(url.path_segments().and_then(|mut segments| segments.next_back()).or_fail()?.parse()?)
}

/// Requires the context details `entry` to carry a non-empty string `URL` and `localId`, the `kind`, a `createdAt`
/// date, a `lastUsage` that is absent or a date, and a `numberOfHits` that is absent or an integer (clause 5.13.3.5).
pub(crate) fn expect_context_details(entry: &Value, kind: ContextKindFilter) {
    expect_that!(entry.get("URL").and_then(Value::as_str), some(not(eq(""))));
    expect_that!(entry.get("localId").and_then(Value::as_str), some(not(eq(""))));
    expect_that!(entry.get("kind"), some(eq(&json!(kind.to_string()))));
    expect_that!(entry.get("createdAt").and_then(Value::as_str).map(is_ngsild_date), some(eq(true)));
    if let Some(last_usage) = entry.get("lastUsage") {
        expect_that!(last_usage.as_str().map(is_ngsild_date), some(eq(true)), "lastUsage is a date");
    }
    if let Some(hits) = entry.get("numberOfHits") {
        expect_that!(hits.is_i64() || hits.is_u64(), eq(true), "numberOfHits is an integer");
    }
}

/// The members a served context's details may carry (clause 5.13.3.5, and `body` and `expiresAt` the keyword allows).
const DETAIL_MEMBERS: [&str; 9] = [
    "URL",
    "localId",
    "kind",
    "createdAt",
    "lastUsage",
    "numberOfHits",
    "extraInfo",
    "body",
    "expiresAt",
];

/// Serve @context with `details=true` of `id`, with `Accept: */*`: requires 200 and answers the response. The request
/// is sent unchecked because the typed serving refuses the `null` `expiresAt` and `extraInfo` the broker writes, which
/// the case does not read.
pub(crate) async fn served_details(client: &BrokerClient, id: &ContextId) -> Result<Response<Bytes>> {
    let client = client.with_accept(Accept::any());
    let request = client.unchecked(&ServeContextDetails { id })?;
    let answer = client.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    Ok(answer?)
}

/// Serve @context with `details=true` of `id` as [`served_details`] does, retried every 250 ms until the delivery deadline until it
/// answers 200 with details that `holds` accepts: the broker records a downloaded context after the request that
/// downloaded it. Answers the details.
pub(crate) async fn eventual_details(client: &BrokerClient, id: &ContextId, holds: impl Fn(&Value) -> bool + Sync) -> Result<Value> {
    let client = client.with_accept(Accept::any());
    Ok(eventually(delivery_deadline(), || async {
        let answer = match client.unchecked(&ServeContextDetails { id }) {
            Ok(request) => client.send_unchecked(request).await,
            Err(error) => return Poll::Pending(error.to_string()),
        };
        match answer.map(|response| (response.status(), serde_json::from_slice::<Value>(response.body()))) {
            Ok((StatusCode::OK, Ok(details))) if holds(&details) => Poll::Ready(details),
            Ok((status, details)) => Poll::Pending(format!("{status} {details:?}")),
            Err(error) => Poll::Pending(error.to_string()),
        }
    })
    .await?)
}

/// The served details `response` as JSON, required to come in `application/json`.
pub(crate) fn details_in_json(response: &Response<Bytes>) -> Result<Value> {
    expect_that!(header_value(response.headers(), &CONTENT_TYPE), some(eq("application/json")));
    Ok(serde_json::from_slice(response.body())?)
}

/// Requires every member of the served details `details` to be one of the detail members.
pub(crate) fn expect_only_detail_members(details: &Value) -> Result<()> {
    let unexpected = details
        .as_object()
        .or_fail()?
        .keys()
        .filter(|member| !DETAIL_MEMBERS.contains(&member.as_str()))
        .collect::<Vec<_>>();
    expect_that!(unexpected, is_empty());
    Ok(())
}
