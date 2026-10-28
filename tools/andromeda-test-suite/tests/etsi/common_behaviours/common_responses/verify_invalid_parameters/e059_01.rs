//! ETSI `059_01`: an operation sent with the undefined parameter `invalidParams=invalidValue` is refused with 400
//! `InvalidRequest` (clause 6.3.20).

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::problem,
    vocabulary::Vocabulary,
    wire::{content_type, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// The media type every case sends.
const LD_JSON: &str = "application/ld+json";

/// Sends `method` to `path` as the case does, with `Content-Type: application/ld+json` and `body`, and checks the
/// refusal: 400 with the problem type `InvalidRequest`.
async fn refused(client: &BrokerClient, method: Method, path: &str, body: Option<String>) -> Result<()> {
    let request = content_type(request_to(client, method, path)?, LD_JSON);
    let request = match body {
        Some(text) => request.body(Bytes::from(text)),
        None => request,
    };
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest))
}

/// The text of the suite's `entities/building-minimal.json` for `urn:ngsi-ld:Building:<ns>-1`, as the one JSON string
/// RequestsLibrary sends when `json=` is given the file's text.
fn building_minimal(namespace: &Namespace) -> Result<String> {
    let text = format!("{{\n    \"id\": \"{}\",\n    \"type\": \"Building\"\n}}", namespace.urn("Building", 1)?);
    Ok(serde_json::to_string(&text)?)
}

/// Pins: Check that sending an invalid parameter returns a 400 invalidRequest
/// Clause: 5.6.1, 6.3.20
/// Changed: the suite posts the text of entities/building-minimal.json as one JSON string (RequestsLibrary json= with
/// a str), so the body is that string, kept as sent
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e059_01_01_create_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    refused(
        &broker,
        Method::POST,
        "entities?invalidParams=invalidValue",
        Some(building_minimal(&namespace)?),
    )
    .await
}

/// Pins: Check that sending an invalid parameter returns a 400 invalidRequest
/// Clause: 5.7.2, 6.3.20
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e059_01_02_query_entities() -> Result<()> {
    refused(&broker::front()?, Method::GET, "entities?invalidParams=invalidValue&type=Building", None).await
}

/// Pins: Check that sending an invalid parameter returns a 400 invalidRequest
/// Clause: 5.6.11, 6.3.20
/// Changed: the suite posts the text of entities/building-minimal.json as one JSON string (RequestsLibrary json= with
/// a str), so the body is that string, kept as sent
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e059_01_03_create_temporal_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    refused(
        &broker,
        Method::POST,
        "temporal/entities?invalidParams=invalidValue",
        Some(building_minimal(&namespace)?),
    )
    .await
}

/// Pins: Check that sending an invalid parameter returns a 400 invalidRequest
/// Clause: 5.7.2, 6.3.20
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e059_01_04_query_temporal_entities() -> Result<()> {
    refused(
        &broker::front()?,
        Method::GET,
        "temporal/entities?invalidParams=invalidValue&type=Building",
        None,
    )
    .await
}

/// Pins: Check that sending an invalid parameter returns a 400 invalidRequest
/// Clause: 5.7.5, 6.3.20
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e059_01_05_retrieve_entity_types() -> Result<()> {
    refused(&broker::front()?, Method::GET, "types?invalidParams=invalidValue", None).await
}

/// Pins: Check that sending an invalid parameter returns a 400 invalidRequest
/// Clause: 5.8.1, 6.3.20
/// Changed: the suite posts the text of subscriptions/subscription.jsonld as one JSON string (RequestsLibrary json=
/// with a str), so the body is that string, kept as sent
#[gtest]
#[tokio::test]
async fn e059_01_06_create_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let text = format!(
        "{{\n   \"id\":\"{}\",\n   \"type\":\"Subscription\",\n   \"timeInterval\": 5,\n   \"entities\":[\n      {{\n         \"type\":\"Building\"\n      }}\n   ],\n   \"notification\":{{\n      \"format\":\"keyValues\",\n      \"endpoint\":{{\n         \"uri\":\"{}\",\n         \"accept\":\"application/json\"\n      }}\n   }},\n   \"@context\":[\n      \"{}\"\n   ]\n}}",
        namespace.urn_text("Subscription", 1),
        sink.endpoint(1)?,
        vocabulary.url(),
    );
    refused(
        &broker,
        Method::POST,
        "subscriptions?invalidParams=invalidValue",
        Some(serde_json::to_string(&text)?),
    )
    .await
}

/// Pins: Check that sending an invalid parameter returns a 400 invalidRequest
/// Clause: 5.8.1, 6.3.20
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e059_01_07_query_subscriptions() -> Result<()> {
    refused(&broker::front()?, Method::GET, "subscriptions?invalidParams=invalidValue", None).await
}

/// Pins: Check that sending an invalid parameter returns a 400 invalidRequest
/// Clause: 5.6.21, 6.4.3.3
/// Tenant: private (the purge names no type, so a broker that failed to refuse it would delete every entity of the
/// shared tenant)
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e059_01_08_purge_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    refused(&tenant, Method::DELETE, "entities?invalidParams=invalidValue", None).await
}
