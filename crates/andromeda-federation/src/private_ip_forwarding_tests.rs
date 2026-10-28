//! What the forwarding client does about endpoints in private address space
//! and about redirects.

use crate::{
    FederationService,
    cache::Cacheability,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    mock_source::{endpoint, received},
    outcome::{ForwardGetOutcome, TransportFailure},
    request::TriggeringRequest,
    source_read::SourceRead,
};
use andromeda_configuration::schema::{federation::FederationConfig, private_ip_blocking::PrivateIpBlocking};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use serde_json::json;
use std::sync::Arc;
use url::Url;
use wiremock::{
    Mock,
    MockServer,
    ResponseTemplate,
    matchers::{method, path},
};

const READ_PATH: &str = "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1";

fn registration(source: &MockServer) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:guarded",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": "inclusive",
        "operations": ["retrieveEntity"],
        "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("a valid registration")
}

async fn read(registration: &ContextSourceRegistration, blocking: PrivateIpBlocking, url: Url) -> ForwardGetOutcome {
    let mut config = FederationConfig::default();
    config.forwarding.private_ip_blocking = blocking;
    let service = FederationService::new(
        Arc::new(FixedRegistrationStore::new(vec![registration.clone()])), // clone: the store owns its registrations
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        ContextSourcePseudonym::new("guarded-broker").expect("a valid alias"),
        &config,
    )
    .expect("the service builds");
    let context = ContextSpec::none();
    service
        .forward_get(
            SourceRead::builder()
                .registration(registration)
                .inbound(&context)
                .triggering(TriggeringRequest::default())
                .build(),
            Ok(url),
            Cacheability::Uncacheable,
        )
        .await
}

async fn answering_source() -> MockServer {
    let source = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(READ_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"})))
        .mount(&source)
        .await;
    source
}

#[tokio::test]
async fn a_source_at_a_loopback_literal_is_not_contacted_when_private_addresses_are_refused() {
    let source = answering_source().await;
    let registration = registration(&source);
    let url = Url::parse(&format!("{}{READ_PATH}", source.uri())).expect("a valid URL");

    let outcome = read(&registration, PrivateIpBlocking::Refused, url).await;

    assert!(matches!(outcome, ForwardGetOutcome::Transport(TransportFailure::Refused(_))));
    assert!(
        outcome.warning(&registration.id).is_some(),
        "the refusal is reported as a warning on the partial answer"
    );
    pretty_assertions::assert_eq!(received(&source).await.len(), 0);
}

#[tokio::test]
async fn a_source_named_by_a_host_resolving_to_loopback_is_not_contacted_when_private_addresses_are_refused() {
    let source = answering_source().await;
    let registration = registration(&source);
    let port = Url::parse(&source.uri()).expect("a valid URI").port().expect("a mock port");
    let url = Url::parse(&format!("http://localhost:{port}{READ_PATH}")).expect("a valid URL");

    let outcome = read(&registration, PrivateIpBlocking::Refused, url).await;

    assert!(matches!(outcome, ForwardGetOutcome::Transport(TransportFailure::Refused(_))));
    pretty_assertions::assert_eq!(received(&source).await.len(), 0);
}

#[tokio::test]
async fn a_source_at_a_loopback_literal_is_contacted_by_default() {
    let source = answering_source().await;
    let registration = registration(&source);
    let url = Url::parse(&format!("{}{READ_PATH}", source.uri())).expect("a valid URL");

    let outcome = read(&registration, PrivateIpBlocking::Allowed, url).await;

    assert!(matches!(outcome, ForwardGetOutcome::Success(_)));
}

#[tokio::test]
async fn a_redirect_from_a_source_is_not_followed_and_fails_that_source() {
    let target = answering_source().await;
    let source = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(READ_PATH))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", format!("{}{READ_PATH}", target.uri())))
        .mount(&source)
        .await;
    let registration = registration(&source);
    let url = Url::parse(&format!("{}{READ_PATH}", source.uri())).expect("a valid URL");

    let outcome = read(&registration, PrivateIpBlocking::Allowed, url).await;

    assert!(matches!(outcome, ForwardGetOutcome::ErrorStatus(_)));
    pretty_assertions::assert_eq!(received(&target).await.len(), 0);
}
