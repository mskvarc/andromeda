//! Router-level tests of Serve @context (clause 5.13.4), of the parameters the
//! `/jsonldContexts` operations accept (clause 6.3.20), and of the payload
//! limit Create @context (clause 5.13.2) applies (clause 6.3.2).

use crate::{
    routes::ContextRegistryRoutes,
    state::{ContextRegistryRouteServices, ContextRegistryStateResolver},
    tenant_context_hosting::TenantContextHosting,
};
use andromeda_context_registry::{
    context_page::ContextPage,
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    fetched_context::FetchedContext,
    insertion::ContextInsertion,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    service::ContextRegistryService,
    store::{ContextRegistryFuture, ContextRegistryStore},
    stored_context::StoredContext,
};
use andromeda_http_extraction::request_json_value_limit::RequestJsonValueLimit;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_cache::DownloadedContext, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId, tenant::TenantName};
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_request::pagination::policy::PaginationPolicy;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::lease::LeasedState, tenant_ref::TenantRef};
use andromeda_test_harness::resident_tenancy::ResidentTenancy;
use axum::{
    Extension,
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header::VARY},
    response::Response,
};
use futures::future::BoxFuture;
use serde_json::{Value, json};
use std::{num::NonZeroUsize, sync::Arc};
use tower::ServiceExt;
use url::Url;

/// A Hosted entry the store holds.
const HOSTED: &str = "urn:ngsi-ld:JsonLdContext:0b6e3b5e-6a53-5bd2-9d5c-1f7d8f3e2a10";

/// A Cached entry the store holds.
const CACHED: &str = "urn:ngsi-ld:JsonLdContext:1c7f4c6f-7b64-4ce3-8e6d-2a8e9f4b3b21";

/// A Hosted entry only tenant `acme` holds.
const ACME_HOSTED: &str = "urn:ngsi-ld:JsonLdContext:3e916e81-9d86-4e05-a08f-4caf116d5d43";

/// An identifier the store does not hold.
const UNKNOWN: &str = "urn:ngsi-ld:JsonLdContext:2d805d70-8c75-4df4-9f7e-3b9fa05c4c32";

/// The document the Hosted entry holds.
fn hosted_document() -> JsonLiteral {
    JsonLiteral::admit(&json!({ "@context": { "Spot": "https://e.org/Spot" } }).to_string()).expect("an admissible literal")
}

/// One tenant's store, holding one Hosted and one Cached entry.
struct TwoEntryStore {
    /// The identifier of the Hosted entry this tenant holds.
    hosted: &'static str,
}

impl TwoEntryStore {
    /// The kind of the entry `identifier` names, if the store holds it.
    fn kind(&self, identifier: &Identifier) -> Option<ContextKind> {
        match identifier.as_str() {
            hosted if hosted == self.hosted => Some(ContextKind::Hosted),
            CACHED => Some(ContextKind::Cached),
            _ => None,
        }
    }
}

impl ContextRegistryStore for TwoEntryStore {
    fn insert<'a>(
        &'a self,
        _identifier: &'a JsonLdContextId,
        _payload: &'a JsonLiteral,
        _serving_url: &'a Url,
        _kind: ContextCreationKind,
    ) -> ContextRegistryFuture<'a, ContextInsertion> {
        Box::pin(async { Ok(ContextInsertion::Inserted) })
    }

    fn list<'a>(&'a self, _query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ContextPage> {
        Box::pin(async {
            Ok(ContextPage {
                contexts: ListedContexts::Summary(Vec::new()),
                boundary: None,
            })
        })
    }

    fn get<'a>(&'a self, identifier: &'a Identifier, _detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>> {
        Box::pin(async move { Ok(self.kind(identifier).map(|_| FetchedContext::Payload(hosted_document()))) })
    }

    fn record_access<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            Ok(match self.kind(identifier) {
                Some(_) => ContextMutation::Applied,
                None => ContextMutation::Missing,
            })
        })
    }

    fn lookup<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, Option<StoredContext>> {
        Box::pin(async move {
            Ok(self.kind(identifier).map(|kind| StoredContext {
                identifier: identifier.clone(), // clone: the looked-up row owns its identifier
                kind,
                source_url: None,
            }))
        })
    }

    fn delete<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async { Ok(ContextMutation::Missing) })
    }

    fn replace_download<'a>(&'a self, _identifier: &'a Identifier, _download: &'a DownloadedContext) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async { Ok(ContextMutation::Missing) })
    }
}

/// Resolves the default tenant and `acme` through a genuine runtime cache,
/// each to its own store.
struct ResidentResolver {
    /// The cache the leases come from, serving the default tenant's hosting.
    tenancy: ResidentTenancy<TenantContextHosting>,
    /// The hosting of tenant `acme`.
    acme: TenantContextHosting,
}

impl ContextRegistryStateResolver for ResidentResolver {
    fn resolve<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
    ) -> BoxFuture<'a, Result<Arc<ContextRegistryRouteServices>, TenancyError>> {
        Box::pin(async move {
            let LeasedState { state, lease } = self.tenancy.resolve(tenant, creation_policy).await?;
            let hosting = match tenant {
                TenantRef::Named(_) => &self.acme,
                TenantRef::UnnamedDefault => &state,
            };
            Ok(Arc::new(ContextRegistryRouteServices::new(
                TenantContextHosting::clone(hosting), // clone: the route services own a handle on the tenant's hosting
                lease,
            )))
        })
    }
}

/// The `/jsonldContexts` routes over the two-entry store, with the pagination
/// policy the API assembly installs on every NGSI-LD request.
fn routes() -> Router {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("an offline engine");
    let hosting = |hosted| {
        TenantContextHosting::new(Arc::new(ContextRegistryService::new(
            Arc::new(TwoEntryStore { hosted }),
            engine.clone(), // clone: each tenant's registry holds the engine
        )))
    };
    let acme: TenantName = "acme".parse().expect("a tenant name");
    let resolver = ResidentResolver {
        tenancy: ResidentTenancy::new(Arc::new(hosting(HOSTED)), vec![acme]),
        acme: hosting(ACME_HOSTED),
    };
    ContextRegistryRoutes::new(Arc::new(resolver))
        .into_router()
        .layer(Extension(PaginationPolicy::CursorDefault))
}

/// The answer to `method` on `uri` with `headers`.
async fn answer(method: Method, uri: &str, headers: &[(&str, &str)]) -> Response {
    let mut request = Request::builder().method(method).uri(uri).header("host", "broker.example");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request.body(Body::empty()).expect("a valid request");
    // A `Router`'s service error type is uninhabited.
    match routes().oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// The `ProblemDetails` `type` a response body names, by its last segment.
async fn problem_type(response: Response) -> Option<String> {
    let body = to_bytes(response.into_body(), usize::MAX).await.expect("a readable body");
    let problem: Value = serde_json::from_slice(&body).ok()?;
    problem["type"].as_str().and_then(|uri| uri.rsplit('/').next()).map(str::to_owned)
}

/// Whether a response marks itself as selected by the tenant header.
fn varies_on_tenant(headers: &HeaderMap) -> bool {
    headers
        .get_all(VARY)
        .iter()
        .any(|value| value.as_bytes().eq_ignore_ascii_case(b"NGSILD-Tenant"))
}

fn serve_path(identifier: &str) -> String {
    format!("/jsonldContexts/{identifier}")
}

/// Clauses 4.14 and 6.3.14: the `NGSILD-Tenant` header names the tenant
/// whose entry is served.
#[tokio::test]
async fn a_header_naming_a_tenant_serves_that_tenants_entry() {
    let response = answer(Method::GET, &serve_path(ACME_HOSTED), &[("NGSILD-Tenant", "acme")]).await;

    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    assert!(varies_on_tenant(response.headers()));
}

/// Without the header the default tenant is read, which does not hold a named
/// tenant's entry.
#[tokio::test]
async fn without_a_header_the_default_tenant_is_read() {
    let named = answer(Method::GET, &serve_path(ACME_HOSTED), &[]).await;
    let default = answer(Method::GET, &serve_path(HOSTED), &[]).await;

    pretty_assertions::assert_eq!(named.status(), StatusCode::NOT_FOUND);
    assert!(varies_on_tenant(named.headers()), "RFC 9111 section 4.1: the 404 depends on the tenant");
    pretty_assertions::assert_eq!(problem_type(named).await.as_deref(), Some("ResourceNotFound"));
    pretty_assertions::assert_eq!(default.status(), StatusCode::OK);
}

/// Clause 5.5.10: a header naming a tenant that does not exist is
/// `NonexistentTenant`.
#[tokio::test]
async fn a_header_naming_an_unknown_tenant_is_nonexistent_tenant() {
    let response = answer(Method::GET, &serve_path(HOSTED), &[("NGSILD-Tenant", "nobody")]).await;

    pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(varies_on_tenant(response.headers()), "RFC 9111 section 4.1: the 404 depends on the tenant");
    pretty_assertions::assert_eq!(problem_type(response).await.as_deref(), Some("NonexistentTenant"));
}

/// RFC 7232 section 5: the precondition is evaluated only for a request that
/// would otherwise have been a 2xx, so `*` changes neither a 404 for an
/// unknown identifier nor a 422 for a Cached entry (clause 5.13.4.4).
#[tokio::test]
async fn a_star_precondition_leaves_an_unknown_or_cached_entry_refused() {
    let unknown = answer(Method::GET, &serve_path(UNKNOWN), &[("If-None-Match", "*")]).await;
    let cached = answer(Method::GET, &serve_path(CACHED), &[("If-None-Match", "*")]).await;

    pretty_assertions::assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert!(varies_on_tenant(unknown.headers()));
    pretty_assertions::assert_eq!(problem_type(unknown).await.as_deref(), Some("ResourceNotFound"));
    pretty_assertions::assert_eq!(cached.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(varies_on_tenant(cached.headers()));
    pretty_assertions::assert_eq!(problem_type(cached).await.as_deref(), Some("OperationNotSupported"));
}

/// RFC 7232 sections 3.2 and 4.1: `*` on a served document is 304, with the
/// tenant `Vary` the 200 carries.
#[tokio::test]
async fn a_star_precondition_on_a_hosted_entry_is_not_modified_with_its_vary() {
    let response = answer(Method::GET, &serve_path(HOSTED), &[("If-None-Match", "*")]).await;

    pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert!(varies_on_tenant(response.headers()));
}

/// RFC 9110 section 9.3.2: HEAD answers the header fields GET does, without
/// the content.
#[tokio::test]
async fn head_answers_the_get_headers_without_a_body() {
    let get = answer(Method::GET, &serve_path(HOSTED), &[]).await;
    let head = answer(Method::HEAD, &serve_path(HOSTED), &[]).await;

    pretty_assertions::assert_eq!(head.status(), get.status());
    for name in ["content-type", "etag", "cache-control", "vary"] {
        pretty_assertions::assert_eq!(head.headers().get(name), get.headers().get(name), "{name}");
    }
    let body = to_bytes(head.into_body(), usize::MAX).await.expect("a readable body");
    assert!(body.is_empty(), "a HEAD answer carries no content: {body:?}");
}

/// Clauses 6.3.14 and 6.3.20: the tenant is named by the header alone, so
/// every operation, Serve included, refuses the former vendor tenant parameter
/// as an undefined parameter, whatever tenant it names.
#[tokio::test]
async fn the_former_tenant_parameter_is_invalid_request_on_every_operation() {
    let serve = format!("{}?andromedaTenant=acme", serve_path(ACME_HOSTED));
    for (method, uri, headers) in [
        (Method::GET, serve.as_str(), vec![]),
        (Method::GET, serve.as_str(), vec![("NGSILD-Tenant", "acme")]),
        (
            Method::POST,
            "/jsonldContexts?andromedaTenant=acme",
            vec![("content-type", "application/json"), ("content-length", "0")],
        ),
        (Method::GET, "/jsonldContexts?andromedaTenant=acme", vec![]),
        (Method::DELETE, "/jsonldContexts/urn:ngsi-ld:JsonLdContext:x?andromedaTenant=acme", vec![]),
    ] {
        let response = answer(method.clone(), uri, &headers).await; // clone: the method labels the assertion
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method} {uri}");
        pretty_assertions::assert_eq!(problem_type(response).await.as_deref(), Some("InvalidRequest"), "{method} {uri}");
    }
}

/// Clause 5.13.3.4 lists every stored `@context`, so the listing reads the
/// pagination parameters every paged collection reads (clause 6.3.10).
#[tokio::test]
async fn the_listing_reads_the_pagination_parameters() {
    for uri in [
        "/jsonldContexts?limit=1",
        "/jsonldContexts?limit=1&offset=2",
        "/jsonldContexts?cursor=",
        "/jsonldContexts?kind=Hosted&details=true&limit=5",
    ] {
        pretty_assertions::assert_eq!(answer(Method::GET, uri, &[]).await.status(), StatusCode::OK, "{uri}");
    }
}

/// The listing is refused by the pagination rules every collection shares:
/// `limit=0` asks for nothing without a count (Table 6.3.10-1), and a
/// cursor beside an offset is contradictory (clause 5.5.9).
#[tokio::test]
async fn the_listing_refuses_what_the_pagination_policy_refuses() {
    for uri in ["/jsonldContexts?limit=0", "/jsonldContexts?cursor=&offset=1", "/jsonldContexts?limit=ten"] {
        let response = answer(Method::GET, uri, &[]).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
    }
}

/// The answer to creating `document` under the published JSON value limit of
/// `values`.
async fn create_under_value_limit(document: &JsonLiteral, values: NonZeroUsize) -> Response {
    let body = document.text().to_owned();
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/jsonldContexts")
        .header("host", "broker.example")
        .header("content-type", "application/json")
        .header("content-length", body.len())
        .body(Body::from(body))
        .expect("a valid request");
    request.extensions_mut().insert(RequestJsonValueLimit::new(JsonValueLimit::new(values)));
    match routes().oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// Clause 6.3.2: a context document holding more JSON values than the
/// published limit is the 413 "Request Entity too large", naming the limit,
/// before the document is parsed or stored.
#[tokio::test]
async fn a_context_document_past_the_json_value_limit_is_a_413() {
    let two = NonZeroUsize::new(2).expect("two is not zero");

    let response = create_under_value_limit(&hosted_document(), two).await;

    pretty_assertions::assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = to_bytes(response.into_body(), usize::MAX).await.expect("a readable body");
    let problem: Value = serde_json::from_slice(&body).expect("a problem detail");
    pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");
    pretty_assertions::assert_eq!(problem["instance"], "/jsonldContexts");
    let detail = problem["detail"].as_str().expect("a detail");
    assert!(detail.contains("2 JSON values"), "{detail}");
}

/// A context document holding exactly the published number of values is
/// created.
#[tokio::test]
async fn a_context_document_at_the_json_value_limit_is_created() {
    let three = NonZeroUsize::new(3).expect("three is not zero");

    let response = create_under_value_limit(&hosted_document(), three).await;

    pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
}
