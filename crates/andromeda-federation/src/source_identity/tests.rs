use crate::{
    FederationService,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    forwarded_query::client_query::ClientQuery,
    local_source::LocalSource,
    mock_source::{answer_in_turn, endpoint, received},
    operation::ForwardingOperation,
    participating_read::ParticipatingRead,
    request::EntityMapInUse,
    source_identity::{ReachedSource, SourceConfiguration},
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use http::{HeaderValue, StatusCode};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::sync::Arc;
use wiremock::{MockServer, ResponseTemplate};

/// The identity resource of the mock source.
const IDENTITY_PATH: &str = "/ngsi-ld/v1/info/sourceIdentity";

/// The extras an Andromeda serving the temporal join advertises.
fn advertising_extras() -> Value {
    json!({"extensions": ["temporalLinkedRetrieval"]})
}

async fn source() -> MockServer {
    MockServer::start().await
}

/// A registration of `Building` entities at `source`, serving `operations`,
/// with `extra` members merged in.
fn registration(source: &MockServer, operations: &[&str], extra: &Value) -> ContextSourceRegistration {
    let mut document = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:identity",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "operations": operations,
        "information": [{"entities": [{"type": "https://example.org/Building"}]}],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    });
    if let (Some(document), Some(extra)) = (document.as_object_mut(), extra.as_object()) {
        document.extend(extra.clone());
    }
    serde_json::from_value(document).expect("a valid registration")
}

fn service(registration: ContextSourceRegistration) -> FederationService {
    let store = Arc::new(FixedRegistrationStore::new(vec![registration]));
    let census = store.census();
    FederationService::new(
        store,
        census,
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        LocalSource::new(
            ContextSourceAliases::new(ContextSourcePseudonym::new("probing-broker").expect("a valid alias")),
            TenantRef::UnnamedDefault,
        ),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

/// A clause 5.2.40 identity carrying `extras`.
fn identity(extras: Option<Value>) -> ResponseTemplate {
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
    ResponseTemplate::new(StatusCode::OK).set_body_raw(serde_json::to_vec(&body).expect("the identity serializes"), "application/json")
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
    answer_in_turn(&source, "GET", IDENTITY_PATH, [identity(Some(advertising_extras()))]).await;
    let service = service(registration(&source, &["retrieveTemporal"], &json!({})));

    assert_eq!(
        configurations(&service).await,
        vec![SourceConfiguration::Described(Some(
            JsonLiteral::admit(&advertising_extras().to_string()).expect("a JSON literal")
        ))]
    );
}

/// Clause 5.15.1.4: a source unable to supply its identity answers
/// `NotImplemented`, and has described nothing.
#[tokio::test]
async fn a_source_without_an_identity_is_undescribed() {
    let source = source().await;
    answer_in_turn(&source, "GET", IDENTITY_PATH, [ResponseTemplate::new(StatusCode::NOT_IMPLEMENTED)]).await;
    let service = service(registration(&source, &["retrieveTemporal"], &json!({})));

    assert_eq!(configurations(&service).await, vec![SourceConfiguration::Undescribed]);
}

/// A source's configuration is asked once and reused, not asked again for
/// every Linked Entity.
#[tokio::test]
async fn an_answer_is_reused_rather_than_asked_again() {
    let source = source().await;
    answer_in_turn(&source, "GET", IDENTITY_PATH, [identity(None)]).await;
    let service = service(registration(&source, &["retrieveTemporal"], &json!({})));

    assert_eq!(configurations(&service).await, vec![SourceConfiguration::Described(None)]);
    assert_eq!(configurations(&service).await, vec![SourceConfiguration::Described(None)]);
    assert_eq!(received(&source).await.iter().filter(|request| request.url.path() == IDENTITY_PATH).count(), 1);
}

/// The identity is asked of the source itself: clause 5.15.1 defines no
/// parameter, so a `localOnly` registration adds no `local=true`, while the
/// registration's tenant still addresses the identity it is asked about
/// (Table 5.2.40-1).
#[tokio::test]
async fn the_identity_is_asked_without_a_local_scope_in_the_registered_tenant() {
    let source = source().await;
    answer_in_turn(&source, "GET", IDENTITY_PATH, [identity(None)]).await;
    let service = service(registration(
        &source,
        &["retrieveTemporal"],
        &json!({"tenant": "district", "management": {"localOnly": true}}),
    ));

    configurations(&service).await;

    let requests = received(&source).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.query(), None);
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
    assert_eq!(received(&source).await.len(), 0);
}

/// The sources one Query Entities of Buildings reaches, as `params` asks.
async fn query_configurations(service: &FederationService, params: &EntityQueryParameters) -> Vec<ReachedSource> {
    let read = ParticipatingRead {
        requested_scope: params.execution_scope(),
        client_query: ClientQuery::EntityQuery(params),
        geo: None,
        operation: ForwardingOperation::QueryEntity,
        entity_map: EntityMapInUse::None,
    };
    service
        .query_configurations(&ContextTarget::Live, &ContextSpec::none(), &read)
        .await
        .expect("the registrations are read")
}

fn buildings(query: &str) -> EntityQueryParameters {
    serde_urlencoded::from_str(query).expect("a valid entity query")
}

/// A query reaches the registrations that take part in it, and each one's
/// source is asked for its identity.
#[tokio::test]
async fn a_query_probes_every_registration_taking_part_in_it() {
    let source = source().await;
    answer_in_turn(&source, "GET", IDENTITY_PATH, [identity(Some(advertising_extras()))]).await;
    let service = service(registration(&source, &["queryEntity"], &json!({})));

    assert_eq!(
        query_configurations(&service, &buildings("type=https%3A%2F%2Fexample.org%2FBuilding")).await,
        vec![ReachedSource {
            registration: "urn:ngsi-ld:ContextSourceRegistration:identity".parse().expect("a registration identifier"),
            configuration: SourceConfiguration::Described(Some(JsonLiteral::admit(&advertising_extras().to_string()).expect("a JSON literal"))),
        }]
    );
}

/// Clause 5.5.13: a query limited to the local scope uses no registration, so
/// no source is asked anything.
#[tokio::test]
async fn a_local_query_reaches_no_source() {
    let source = source().await;
    let service = service(registration(&source, &["queryEntity"], &json!({})));

    assert_eq!(
        query_configurations(&service, &buildings("type=https%3A%2F%2Fexample.org%2FBuilding&local=true")).await,
        Vec::new()
    );
}

/// Clause 4.20: a registration that does not serve the query takes no part
/// in it.
#[tokio::test]
async fn a_registration_not_serving_the_query_is_not_reached() {
    let source = source().await;
    let service = service(registration(&source, &["retrieveTemporal"], &json!({})));

    assert_eq!(
        query_configurations(&service, &buildings("type=https%3A%2F%2Fexample.org%2FBuilding")).await,
        Vec::new()
    );
}
