//! Every extractor names the URI the client addressed as its `instance`.
//!
//! The broker mounts each API family under a nested router, and nesting strips
//! the mount prefix from the URI a route sees. An extractor that reported that
//! stripped URI answered `GET /ngsi-ld/v1/entities?q=bad((` with an `instance`
//! of `/?q=bad((`, a relative reference that resolves to no resource at all
//! (IETF RFC 7807 section 3.1, ETSI GS CIM 009 v1.9.1 clause 5.5.3). Each case
//! below mounts one extractor under `/ngsi-ld/v1`, sends it a request it must
//! refuse, and requires the refusal to name the request exactly as sent.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_extraction::{
    api_root::ApiRoot,
    content_length_precondition::DeclaredBodyLength,
    context::ContextUrl,
    context_target::{ContextTargetProvider, ContextTargetResolutionError, ExistingTargetedTenantState, SnapshotTargetResolver},
    entity_batch::BatchBody,
    entity_query::EntityQuery,
    jsonld::JsonLd,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
    snapshot::SnapshotHeader,
    snapshot_purge::SnapshotPurgeQ,
    snapshot_scope_guard::reject_snapshot_scope,
    temporal_delete_query::TemporalDeleteQuery,
    temporal_query::TemporalQuery,
    tenant::TenantHeader,
    tenant_state::{ExistingTenantState, TenantStateResolver},
    typed_path::TypedPath,
};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, tenant_ref::TenantRef};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, Uri},
    middleware::from_fn,
    routing::{delete, get, post},
};
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::Value;
use std::error::Error;
use tower::ServiceExt;

/// The prefix the broker mounts its NGSI-LD API under.
const API_ROOT: &str = "/ngsi-ld/v1";

/// A query parameter set that admits one parameter and refuses every other.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DetailsOnly {
    /// The one parameter the set admits.
    #[serde(rename = "details")]
    _details: Option<bool>,
}

/// Tenant resolution that refuses every tenant.
#[derive(Clone)]
struct RefusingTenants;

impl TenantStateResolver<()> for RefusingTenants {
    fn resolve_tenant<'a>(
        &'a self,
        _tenant: &'a TenantRef,
        _creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
    ) -> BoxFuture<'a, Result<(), ProblemResponse>> {
        Box::pin(async move { Err(ClassifiedReport::public(NgsiLdErrorType::NonexistentTenant, "the tenant does not exist").with_instance(instance.clone())) })
    }
}

/// Tenant state whose live branch exists and whose every Snapshot is unknown.
struct SnapshotlessTenant {
    /// The live branch the tenant serves.
    live: ContextTarget,
}

impl SnapshotTargetResolver for SnapshotlessTenant {
    fn resolve<'a>(&'a self, snapshot_id: &'a Identifier) -> BoxFuture<'a, Result<ContextTarget, ContextTargetResolutionError>> {
        Box::pin(async move {
            Err(ContextTargetResolutionError::NotFound {
                identifier: snapshot_id.clone(),
            })
        })
    }
}

impl ContextTargetProvider for SnapshotlessTenant {
    fn live_target(&self) -> &ContextTarget {
        &self.live
    }

    fn snapshot_target_resolver(&self) -> &dyn SnapshotTargetResolver {
        self
    }
}

/// Tenant resolution that admits every tenant, none of which has a Snapshot.
#[derive(Clone)]
struct AdmittingTenants;

impl TenantStateResolver<SnapshotlessTenant> for AdmittingTenants {
    fn resolve_tenant<'a>(
        &'a self,
        _tenant: &'a TenantRef,
        _creation_policy: TenantCreationPolicy,
        _instance: &'a Uri,
    ) -> BoxFuture<'a, Result<SnapshotlessTenant, ProblemResponse>> {
        Box::pin(async move { Ok(SnapshotlessTenant { live: ContextTarget::Live }) })
    }
}

async fn entity_query(_query: EntityQuery) -> StatusCode {
    StatusCode::OK
}

async fn entity_body(_body: JsonLd<Entity>) -> StatusCode {
    StatusCode::OK
}

async fn entity_path(_id: TypedPath<Identifier>) -> StatusCode {
    StatusCode::OK
}

async fn temporal_query(_query: TemporalQuery) -> StatusCode {
    StatusCode::OK
}

async fn temporal_delete(_query: TemporalDeleteQuery) -> StatusCode {
    StatusCode::OK
}

async fn validated_query(_query: ValidatedQuery<DetailsOnly>) -> StatusCode {
    StatusCode::OK
}

async fn snapshot_purge(_query: SnapshotPurgeQ) -> StatusCode {
    StatusCode::OK
}

async fn protocol_headers(_headers: NgsiLdProtocolHeaders) -> StatusCode {
    StatusCode::OK
}

async fn tenant(_tenant: TenantHeader) -> StatusCode {
    StatusCode::OK
}

async fn snapshot(_snapshot: SnapshotHeader) -> StatusCode {
    StatusCode::OK
}

async fn context(_context: ContextUrl) -> StatusCode {
    StatusCode::OK
}

async fn declared_length(_length: DeclaredBodyLength) -> StatusCode {
    StatusCode::OK
}

async fn batch(_batch: BatchBody) -> StatusCode {
    StatusCode::OK
}

async fn origin(_api_root: ApiRoot) -> StatusCode {
    StatusCode::OK
}

async fn existing_tenant(_tenant: ExistingTenantState<()>) -> StatusCode {
    StatusCode::OK
}

async fn targeted_tenant(_tenant: ExistingTargetedTenantState<SnapshotlessTenant>) -> StatusCode {
    StatusCode::OK
}

/// The URI the envelope names, for the handler that reports its own problems.
async fn envelope_uri(envelope: NgsiLdRequestEnvelope) -> String {
    envelope.uri.to_string()
}

/// The broker's shape: every extractor behind a router nested under the API
/// root, as the composed API mounts them.
fn nested_api() -> Router {
    let api = Router::new()
        .route("/entities", get(entity_query).post(entity_body))
        .route("/entities/{id}", get(entity_path))
        .route("/temporal/entityMaps", get(temporal_query))
        .route("/temporal/entities", delete(temporal_delete))
        .route("/types", get(validated_query))
        .route("/snapshots", delete(snapshot_purge))
        .route("/attributes", get(protocol_headers))
        .route("/subscriptions", get(tenant))
        .route("/csourceSubscriptions", get(snapshot))
        .route("/csourceRegistrations", get(context))
        .route("/entityOperations/merge", post(envelope_uri))
        .route("/jsonldContexts", post(declared_length))
        .route("/entityOperations/upsert", post(batch))
        .route("/info/sourceIdentity", get(origin))
        .route("/tenants/current", get(existing_tenant).with_state(RefusingTenants))
        .route("/entities/{id}/attrs", get(targeted_tenant).with_state(AdmittingTenants))
        .route("/info/health", get(|| async { StatusCode::OK }).layer(from_fn(reject_snapshot_scope)));
    Router::new().nest(API_ROOT, api)
}

/// A request to `path` under the API root, with `headers` and `body`.
fn request(method: Method, path: &str, headers: &[(&str, &str)], body: &'static str) -> Result<Request<Body>, Box<dyn Error>> {
    let mut builder = Request::builder().method(method).uri(format!("{API_ROOT}{path}"));
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    Ok(builder.body(Body::from(body))?)
}

/// A request each extractor must refuse, one per extractor.
fn refused_requests() -> Result<Vec<Request<Body>>, Box<dyn Error>> {
    let malformed_link = [("link", "this is not a link header")];
    Ok(vec![
        request(Method::GET, "/entities?type=Vehicle&limit=abc", &[], "")?,
        request(Method::POST, "/entities", &[("content-type", "application/json"), ("content-length", "1")], "{")?,
        request(Method::GET, "/entities/bare-name", &[], "")?,
        request(Method::GET, "/temporal/entityMaps?type=Vehicle&lastN=abc", &[], "")?,
        request(Method::DELETE, "/temporal/entities?type=Vehicle&lastN=5", &[], "")?,
        request(Method::GET, "/types?unknownParameter=1", &[], "")?,
        request(Method::DELETE, "/snapshots", &[], "")?,
        request(Method::GET, "/attributes?details=true", &[("ngsild-snapshot", "not-a-urn")], "")?,
        request(Method::GET, "/subscriptions?limit=5", &[("ngsild-tenant", "")], "")?,
        request(Method::GET, "/csourceSubscriptions", &[("ngsild-snapshot", "not-a-urn")], "")?,
        request(Method::GET, "/csourceRegistrations?type=Vehicle", &malformed_link, "")?,
        request(Method::POST, "/jsonldContexts", &[("content-length", "abc")], "")?,
        request(
            Method::POST,
            "/entityOperations/upsert?options=update",
            &[
                ("content-type", "application/ld+json"),
                ("content-length", "2"),
                ("link", r#"<https://example.org/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#),
            ],
            "[]",
        )?,
        request(Method::GET, "/info/sourceIdentity", &[("host", "broker.example:99999")], "")?,
        request(Method::GET, "/tenants/current", &[("ngsild-tenant", "missing")], "")?,
        request(
            Method::GET,
            "/entities/urn:ngsi-ld:Vehicle:1/attrs",
            &[("ngsild-snapshot", "urn:ngsi-ld:Snapshot:missing")],
            "",
        )?,
        request(Method::GET, "/info/health", &[("ngsild-snapshot", "urn:ngsi-ld:Snapshot:1")], "")?,
    ])
}

/// The status and problem details body a nested route answers `request` with.
async fn answer(request: Request<Body>) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let response = nested_api().oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    Ok((status, serde_json::from_slice(&body)?))
}

#[tokio::test]
async fn every_extractor_refusal_names_the_uri_the_client_addressed() -> Result<(), Box<dyn Error>> {
    // Every case is answered before anything is asserted, so a failure
    // names every extractor it reaches rather than only the first.
    let mut misnamed = Vec::new();
    for request in refused_requests()? {
        let addressed = format!("{} {}", request.method(), request.uri());
        let expected_instance = request.uri().to_string();

        let (status, problem) = answer(request).await?;

        assert!(status.is_client_error(), "{addressed} was not refused: {status} {problem}");
        if problem["instance"] != expected_instance.as_str() {
            misnamed.push(format!("{addressed} named {}", problem["instance"]));
        }
    }
    assert!(misnamed.is_empty(), "refusals not naming the addressed URI:\n{}", misnamed.join("\n"));
    Ok(())
}

/// The envelope carries the URI a handler reports its own problems against,
/// so it must name the request as addressed too.
#[tokio::test]
async fn the_request_envelope_carries_the_uri_the_client_addressed() -> Result<(), Box<dyn Error>> {
    let request = request(Method::POST, "/entityOperations/merge?options=noOverwrite", &[], "")?;

    let response = nested_api().oneshot(request).await?;

    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    pretty_assertions::assert_eq!(body.as_ref(), b"/ngsi-ld/v1/entityOperations/merge?options=noOverwrite".as_slice());
    Ok(())
}
