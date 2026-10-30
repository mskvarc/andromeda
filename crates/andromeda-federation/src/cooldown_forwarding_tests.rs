//! The cooldown a failed read leaves on a write or a subscription forward to
//! the same registration (Table 5.2.34-1, clause 6.3.17).

use crate::{
    FederationService,
    acted_on_answer::ActedOnAnswer,
    cache::Cacheability,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    forwarded_write::{ForwardWrite, WritePayload},
    mock_source::{endpoint, received},
    outcome::{ForwardGetOutcome, ForwardedWriteOutcome, TransportFailure},
    request::TriggeringRequest,
    source_read::SourceRead,
    subscription::{SubscriptionForward, SubscriptionForwardAdmission},
    subscription_forward_failure::SubscriptionForwardFailure,
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription_request::SubscriptionRequest};
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

const SUBSCRIPTIONS_PATH: &str = "/ngsi-ld/v1/subscriptions";

const STALL: Duration = Duration::from_millis(600);

fn registration(source: &MockServer, management: &serde_json::Value) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:cooling",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": "inclusive",
        "operations": ["retrieveEntity", "deleteEntity", "createSubscription", "updateSubscription", "deleteSubscription"],
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

fn read_url(source: &MockServer) -> Url {
    Url::parse(&format!("{}{READ_PATH}", source.uri())).expect("a valid URL")
}

fn remote_copy() -> Identifier {
    "urn:ngsi-ld:Subscription:copy-1".parse().expect("a valid URI")
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
    Mock::given(method("POST"))
        .and(path(SUBSCRIPTIONS_PATH))
        .respond_with(ResponseTemplate::new(StatusCode::CREATED))
        .mount(&source)
        .await;
    source
}

async fn timed_out_read(service: &FederationService, source: &MockServer, registration: &ContextSourceRegistration) {
    let context = ContextSpec::none();
    let read = service
        .forward_get(
            SourceRead::builder()
                .registration(registration)
                .inbound(&context)
                .triggering(TriggeringRequest::default())
                .build(),
            Ok(read_url(source)),
            Cacheability::Uncacheable,
        )
        .await;
    assert!(matches!(read, ForwardGetOutcome::Transport(TransportFailure::TimedOut { .. })));
}

async fn read_then_write(source: &MockServer, registration: &ContextSourceRegistration) -> ForwardedWriteOutcome {
    let service = service(registration);
    timed_out_read(&service, source, registration).await;
    let context = ContextSpec::none();
    service
        .forward_write(ForwardWrite {
            registration,
            url: Ok(read_url(source)),
            method: reqwest::Method::DELETE,
            payload: WritePayload::Empty(&context),
            triggering: TriggeringRequest::default(),
            linked_map: None,
            acted_on: ActedOnAnswer::NoErrorAnswer,
        })
        .await
}

#[tokio::test]
async fn a_write_after_a_timed_out_read_reaches_a_registration_that_sets_no_cooldown() {
    let source = stalling_source().await;
    let registration = registration(&source, &json!({"timeout": 100}));
    let write = read_then_write(&source, &registration).await;
    assert!(matches!(write, ForwardedWriteOutcome::Completed { status: 204, .. }), "{write:?}");
    pretty_assertions::assert_eq!(received(&source).await.len(), 2);
}

#[tokio::test]
async fn a_write_after_a_timed_out_read_is_declined_by_a_registration_that_sets_a_cooldown() {
    let source = stalling_source().await;
    let registration = registration(&source, &json!({"timeout": 100, "cooldown": 60_000}));
    let write = read_then_write(&source, &registration).await;
    assert!(
        matches!(write, ForwardedWriteOutcome::Transport(TransportFailure::CoolingDown { .. })),
        "{write:?}"
    );
    pretty_assertions::assert_eq!(received(&source).await.len(), 1);
}

#[tokio::test]
async fn a_subscription_copy_after_a_timed_out_read_reaches_a_registration_that_sets_no_cooldown() {
    let source = stalling_source().await;
    let registration = registration(&source, &json!({"timeout": 100}));
    let service = service(&registration);
    timed_out_read(&service, &source, &registration).await;
    let copy = SubscriptionRequest::default();
    let context = ContextSpec::none();
    let forwarded = service
        .forward_subscription(
            &registration,
            SubscriptionForward::Create {
                body: &copy,
                context: &context,
            },
        )
        .await;
    assert!(
        matches!(forwarded.outcome, Some(ForwardedWriteOutcome::Completed { status: 201, .. })),
        "{:?}",
        forwarded.outcome
    );
    pretty_assertions::assert_eq!(received(&source).await.len(), 2);
}

#[tokio::test]
async fn every_subscription_forward_after_a_timed_out_read_is_declined_by_a_registration_that_sets_a_cooldown() {
    let source = stalling_source().await;
    let registration = registration(&source, &json!({"timeout": 100, "cooldown": 60_000}));
    let service = service(&registration);
    timed_out_read(&service, &source, &registration).await;
    let copy = SubscriptionRequest::default();
    let context = ContextSpec::none();
    let remote = remote_copy();
    let forwards = [
        SubscriptionForward::Create {
            body: &copy,
            context: &context,
        },
        SubscriptionForward::Update {
            remote_id: &remote,
            body: &copy,
            context: &context,
        },
        SubscriptionForward::Delete { remote_id: &remote },
    ];
    for forward in forwards {
        let forwarded = service.forward_subscription(&registration, forward).await;
        pretty_assertions::assert_eq!(forwarded.admission, SubscriptionForwardAdmission::Forwarded);
        assert!(
            matches!(
                forwarded.failure(),
                Some(SubscriptionForwardFailure::Transport(TransportFailure::CoolingDown { .. }))
            ),
            "{:?}",
            forwarded.outcome
        );
    }
    pretty_assertions::assert_eq!(received(&source).await.len(), 1);
}
