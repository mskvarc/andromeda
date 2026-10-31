//! The clause 5.6.8.4 follow-up of a batch upsert driven entity by entity,
//! against a source that holds itself back after a failure (Table 5.2.34-1).

use crate::{
    FederationService,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    local_source::LocalSource,
    mock_source::{answer_in_turn, endpoint, received},
    outcome::{ForwardedWriteOutcome, TransportFailure},
    provision_batch::{BatchEntity, BatchForwardedResult, BatchProvisionOperation, ForwardBatchRequest, registration_fixture::document},
    request::TriggeringRequest,
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{batch_contexts::BatchContext, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::options::UpsertMode;
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use http::StatusCode;
use serde_json::json;
use std::sync::Arc;
use wiremock::{MockServer, ResponseTemplate};

const ENTITY: &str = "urn:ngsi-ld:Vehicle:1";

const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";

const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";

const ENTITIES_PATH: &str = "/ngsi-ld/v1/entities";

const ENTITY_PATH: &str = "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1";

const ATTRS_PATH: &str = "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1/attrs";

/// A registration of `source` advertising `operations`, with a cooldown of
/// its own, so a failure holds it back under the broker's default policy.
fn registration(source: &MockServer, operations: &[&str]) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:upsert",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": "inclusive",
        "operations": operations,
        "information": [{"entities": [{"type": VEHICLE}]}],
        "management": {"cooldown": 60_000},
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("a valid registration")
}

fn service(registration: &ContextSourceRegistration) -> FederationService {
    FederationService::new(
        Arc::new(FixedRegistrationStore::new(vec![registration.clone()])), // clone: the store owns its registrations
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        LocalSource::new(
            ContextSourceAliases::new(ContextSourcePseudonym::new("upsert-broker").expect("a valid alias")),
            TenantRef::UnnamedDefault,
        ),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

async fn forward(service: &FederationService, operation: BatchProvisionOperation) -> Vec<BatchForwardedResult> {
    let id: Identifier = ENTITY.parse().expect("a valid URI");
    let document = document(ENTITY, VEHICLE, &[SPEED]);
    let context = BatchContext::shared(ContextSpec::none());
    let entities = [BatchEntity {
        id: &id,
        document: Some(&document),
        context: &context,
    }];
    service
        .forward_batch(
            &ContextTarget::Live,
            ForwardBatchRequest {
                operation,
                entities: &entities,
                query_string: None,
                triggering: TriggeringRequest::default(),
            },
        )
        .await
        .expect("the registrations are read")
}

async fn requests(source: &MockServer) -> Vec<(String, String)> {
    received(source)
        .await
        .iter()
        .map(|request| (request.method.to_string(), request.url.path().to_owned()))
        .collect()
}

fn sent(method: &str, path: &str) -> (String, String) {
    (method.to_owned(), path.to_owned())
}

#[tokio::test]
async fn an_already_exists_answer_to_the_create_is_followed_by_the_replace_entity() {
    let source = MockServer::start().await;
    answer_in_turn(&source, "POST", ENTITIES_PATH, [ResponseTemplate::new(StatusCode::CONFLICT)]).await;
    answer_in_turn(&source, "PUT", ENTITY_PATH, [ResponseTemplate::new(StatusCode::NO_CONTENT)]).await;
    let registration = registration(&source, &["createEntity", "replaceEntity"]);
    let results = forward(&service(&registration), BatchProvisionOperation::Upsert(UpsertMode::Replace)).await;
    pretty_assertions::assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0].outcome, ForwardedWriteOutcome::Completed { status: 204, .. }),
        "{:?}",
        results[0].outcome
    );
    pretty_assertions::assert_eq!(requests(&source).await, [sent("POST", ENTITIES_PATH), sent("PUT", ENTITY_PATH)]);
}

#[tokio::test]
async fn an_already_exists_answer_to_the_create_is_followed_by_the_update_attributes_in_update_mode() {
    let source = MockServer::start().await;
    answer_in_turn(&source, "POST", ENTITIES_PATH, [ResponseTemplate::new(StatusCode::CONFLICT)]).await;
    answer_in_turn(&source, "PATCH", ATTRS_PATH, [ResponseTemplate::new(StatusCode::NO_CONTENT)]).await;
    let registration = registration(&source, &["createEntity", "updateEntity"]);
    let results = forward(&service(&registration), BatchProvisionOperation::Upsert(UpsertMode::Update)).await;
    pretty_assertions::assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0].outcome, ForwardedWriteOutcome::Completed { status: 204, .. }),
        "{:?}",
        results[0].outcome
    );
    pretty_assertions::assert_eq!(requests(&source).await, [sent("POST", ENTITIES_PATH), sent("PATCH", ATTRS_PATH)]);
}

#[tokio::test]
async fn an_already_exists_answer_to_a_plain_batch_create_still_holds_the_registration_back() {
    let source = MockServer::start().await;
    answer_in_turn(&source, "POST", ENTITIES_PATH, [ResponseTemplate::new(StatusCode::CONFLICT)]).await;
    let registration = registration(&source, &["createEntity"]);
    let service = service(&registration);
    let first = forward(&service, BatchProvisionOperation::Create).await;
    assert!(
        matches!(first[0].outcome, ForwardedWriteOutcome::RemoteHttpFailure { status: 409, .. }),
        "{:?}",
        first[0].outcome
    );
    let second = forward(&service, BatchProvisionOperation::Create).await;
    assert!(
        matches!(second[0].outcome, ForwardedWriteOutcome::Transport(TransportFailure::CoolingDown { .. })),
        "{:?}",
        second[0].outcome
    );
    pretty_assertions::assert_eq!(requests(&source).await, [sent("POST", ENTITIES_PATH)]);
}
