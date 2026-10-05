use crate::{
    FederationService,
    config::FederationConfig,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    source_identity::SourceConfiguration,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::MockResponse,
    context_source_server::ContextSourceServer,
    server_config::MockServerConfig,
};
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use url::Host;

/// The identity resource of the mock source.
const IDENTITY_PATH: &str = "/ngsi-ld/v1/info/sourceIdentity";

/// The extras an Andromeda serving the temporal join advertises.
fn advertising_extras() -> Value {
    json!({"extensions": ["temporal-linked-retrieval"]})
}

async fn source() -> ContextSourceServer {
    let config = MockServerConfig::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Host::Ipv4(Ipv4Addr::LOCALHOST));
    ContextSourceServer::start(config).await.expect("the mock source starts")
}

fn path(raw: &str) -> CallbackPath {
    CallbackPath::parse(raw).expect("an absolute path")
}

/// A registration of `Building` entities at `source`, serving `operations`,
/// with `extra` members merged in.
fn registration(source: &ContextSourceServer, operations: &[&str], extra: &Value) -> ContextSourceRegistration {
    let mut document = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:identity",
        "type": "ContextSourceRegistration",
        "endpoint": source.endpoint(&path("/")).expect("the endpoint resolves").as_str(),
        "operations": operations,
        "information": [{"entities": [{"type": "https://example.org/Building"}]}],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    });
    if let (Some(document), Some(extra)) = (document.as_object_mut(), extra.as_object()) {
        document.extend(extra.clone()); // clone: the registration owns the members it is built with
    }
    serde_json::from_value(document).expect("a valid registration")
}

fn service(registration: ContextSourceRegistration) -> FederationService {
    FederationService::new(
        Arc::new(FixedRegistrationStore::new(vec![registration])),
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        ContextSourcePseudonym::new("probing-broker").expect("a valid alias"),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

/// A clause 5.2.40 identity carrying `extras`.
fn identity(extras: Option<Value>) -> MockResponse {
    let mut body = json!({
        "id": "urn:ngsi-ld:ContextSourceIdentity:peer",
        "type": "ContextSourceIdentity",
        "contextSourceAlias": "peer",
        "contextSourceUptime": "PT1H",
        "contextSourceTimeAt": "2024-01-01T00:00:00Z"
    });
    if let Some(extras) = extras {
        body["contextSourceExtras"] = extras;
    }
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    MockResponse::builder()
        .status(StatusCode::OK)
        .headers(headers)
        .body(Bytes::from(serde_json::to_vec(&body).expect("the identity serializes")))
        .build()
}

fn building() -> Identifier {
    "urn:ngsi-ld:Building:B1".parse().expect("valid URI")
}

async fn configurations(service: &FederationService) -> Vec<SourceConfiguration> {
    service
        .temporal_retrieval_configurations(&ContextTarget::Live, &building(), &ContextSpec::none())
        .await
        .expect("the registrations are read")
}

/// Clause 5.15.1: a source answering its identity is described by its
/// `contextSourceExtras` (Table 5.2.40-1).
#[tokio::test]
async fn a_source_answering_its_identity_is_described_by_its_extras() {
    let source = source().await;
    source.enqueue(Method::GET, path(IDENTITY_PATH), identity(Some(advertising_extras()))).await;
    let service = service(registration(&source, &["retrieveTemporal"], &json!({})));

    assert_eq!(configurations(&service).await, vec![SourceConfiguration::Described(Some(advertising_extras()))]);
}

/// Clause 5.15.1.4: a source unable to supply its identity answers
/// `NotImplemented`, and has described nothing.
#[tokio::test]
async fn a_source_without_an_identity_is_undescribed() {
    let source = source().await;
    source
        .enqueue(
            Method::GET,
            path(IDENTITY_PATH),
            MockResponse::builder().status(StatusCode::NOT_IMPLEMENTED).build(),
        )
        .await;
    let service = service(registration(&source, &["retrieveTemporal"], &json!({})));

    assert_eq!(configurations(&service).await, vec![SourceConfiguration::Undescribed]);
}

/// A source's configuration is asked once and reused, not asked again for
/// every Linked Entity.
#[tokio::test]
async fn an_answer_is_reused_rather_than_asked_again() {
    let source = source().await;
    source.enqueue(Method::GET, path(IDENTITY_PATH), identity(None)).await;
    let service = service(registration(&source, &["retrieveTemporal"], &json!({})));

    assert_eq!(configurations(&service).await, vec![SourceConfiguration::Described(None)]);
    assert_eq!(configurations(&service).await, vec![SourceConfiguration::Described(None)]);
    assert_eq!(source.request_count(Method::GET, path(IDENTITY_PATH)), 1);
}

/// The identity is asked of the source itself: clause 5.15.1 defines no
/// parameter, so a `localOnly` registration adds no `local=true`, while the
/// registration's tenant still addresses the identity it is asked about
/// (Table 5.2.40-1).
#[tokio::test]
async fn the_identity_is_asked_without_a_local_scope_in_the_registered_tenant() {
    let source = source().await;
    source.enqueue(Method::GET, path(IDENTITY_PATH), identity(None)).await;
    let service = service(registration(
        &source,
        &["retrieveTemporal"],
        &json!({"tenant": "district", "management": {"localOnly": true}}),
    ));

    configurations(&service).await;

    let requests = source.drain_requests().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].uri.query(), None);
    assert_eq!(
        requests[0].headers.get("ngsild-tenant").map(HeaderValue::as_bytes),
        Some(b"district".as_slice())
    );
}

/// Only the sources the forwarded temporal retrieval would reach are asked:
/// a registration not serving it is not contacted at all.
#[tokio::test]
async fn a_registration_not_serving_the_temporal_retrieval_is_not_asked() {
    let source = source().await;
    let service = service(registration(&source, &["retrieveEntity"], &json!({})));

    assert_eq!(configurations(&service).await, Vec::new());
    assert_eq!(source.total_request_count(), 0);
}
