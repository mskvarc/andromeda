//! The cooldown a failed read leaves on a write to the same registration
//! (Table 5.2.34-1, clause 6.3.17).

use crate::{
    FederationService,
    cache::Cacheability,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    forwarded_write::{ForwardWrite, WritePayload},
    mock_source::{endpoint, received},
    outcome::{ForwardGetOutcome, ForwardedWriteOutcome, TransportFailure},
    request::TriggeringRequest,
    source_read::SourceRead,
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use http::StatusCode;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use url::Url;
use wiremock::{
    Mock,
    MockServer,
    ResponseTemplate,
    matchers::{method, path},
};

const READ_PATH: &str = "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1";

const STALL: Duration = Duration::from_millis(600);

fn registration(source: &MockServer, management: &serde_json::Value) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:cooling",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": "inclusive",
        "operations": ["retrieveEntity", "deleteEntity"],
        "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
        "management": management,
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
        ContextSourcePseudonym::new("cooling-broker").expect("a valid alias"),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

async fn stalling_source() -> MockServer {
    let source = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(READ_PATH))
        .respond_with(ResponseTemplate::new(StatusCode::OK).set_delay(STALL))
        .mount(&source)
        .await;
    Mock::given(method("DELETE"))
        .and(path(READ_PATH))
        .respond_with(ResponseTemplate::new(StatusCode::NO_CONTENT))
        .mount(&source)
        .await;
    source
}

async fn read_then_write(source: &MockServer, registration: &ContextSourceRegistration) -> (ForwardGetOutcome, ForwardedWriteOutcome) {
    let service = service(registration);
    let url = Url::parse(&format!("{}{READ_PATH}", source.uri())).expect("a valid URL");
    let context = ContextSpec::none();
    let read = service
        .forward_get(
            SourceRead::builder()
                .registration(registration)
                .inbound(&context)
                .triggering(TriggeringRequest::default())
                .build(),
            Ok(url.clone()), // clone: the write goes to the same resource
            Cacheability::Uncacheable,
        )
        .await;
    let write = service
        .forward_write(ForwardWrite {
            registration,
            url: Ok(url),
            method: reqwest::Method::DELETE,
            payload: WritePayload::Empty(&context),
            triggering: TriggeringRequest::default(),
            linked_map: None,
        })
        .await;
    (read, write)
}

#[tokio::test]
async fn a_write_after_a_timed_out_read_reaches_a_registration_that_sets_no_cooldown() {
    let source = stalling_source().await;
    let registration = registration(&source, &json!({"timeout": 100}));
    let (read, write) = read_then_write(&source, &registration).await;
    assert!(matches!(read, ForwardGetOutcome::Transport(TransportFailure::TimedOut { .. })));
    assert!(matches!(write, ForwardedWriteOutcome::Completed { status: 204, .. }), "{write:?}");
    pretty_assertions::assert_eq!(received(&source).await.len(), 2);
}

#[tokio::test]
async fn a_write_after_a_timed_out_read_is_declined_by_a_registration_that_sets_a_cooldown() {
    let source = stalling_source().await;
    let registration = registration(&source, &json!({"timeout": 100, "cooldown": 60_000}));
    let (read, write) = read_then_write(&source, &registration).await;
    assert!(matches!(read, ForwardGetOutcome::Transport(TransportFailure::TimedOut { .. })));
    assert!(
        matches!(write, ForwardedWriteOutcome::Transport(TransportFailure::CoolingDown { .. })),
        "{write:?}"
    );
    pretty_assertions::assert_eq!(received(&source).await.len(), 1);
}
