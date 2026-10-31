//! The temporal evolution of an entity with a `LanguageProperty` is retrieved in the normalized and in the simplified
//! temporal representation, with the vocabulary named in the `Link` header (clauses 5.7.3, 4.5.7, 4.5.9 and 4.5.18).
//!
//! The answer is compared exactly with the test's expectation.

use crate::temporal_operations::create_temporal_evolution;
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Input, LanguageMap, LanguageProperty, evolution::TemporalEntity},
};
use serde_json::json;

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the case: its `street`, a `LanguageProperty` observed at 12:03, 12:05
/// and 12:07 on 2020-08-01 in UTC, with one more language in each later text.
fn vehicle_with_street(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?)
                .observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([
                ("fr", "Grand Place"),
                ("nl", "Grote Markt"),
                ("es", "Gran Mercado"),
            ])?)
            .observed_at("2020-08-01T12:05:00Z".parse()?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([
                ("fr", "Grand Marché"),
                ("nl", "Grote Markt"),
                ("es", "Gran Lugar"),
                ("it", "Grande Mercato"),
            ])?)
            .observed_at("2020-08-01T12:07:00Z".parse()?),
        )
        .build()?)
}

/// Creates the test's Vehicle, and answers the vocabulary it is stored under.
async fn created_vehicle(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    create_temporal_evolution(broker, namespace, &["Vehicle", "street"], &vehicle_with_street(namespace)?).await
}

/// Pins: Check that one can retrieve the temporal evolution of an entity with a `LanguageProperty` property
/// Clause: 5.7.3, 4.5.7, 4.5.18
#[gtest]
#[tokio::test]
async fn e020_12_01_retrieve_the_normalized_temporal_representation_of_an_entity_with_a_language_property_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve(&id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "street": [
            {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
                "observedAt": "2020-08-01T12:03:00Z",
            },
            {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt", "es": "Gran Mercado"},
                "observedAt": "2020-08-01T12:05:00Z",
            },
            {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Marché", "nl": "Grote Markt", "es": "Gran Lugar", "it": "Grande Mercato"},
                "observedAt": "2020-08-01T12:07:00Z",
            },
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can retrieve the temporal evolution of an entity with a `LanguageProperty` property
/// Clause: 5.7.3, 4.5.9, 4.5.18
#[gtest]
#[tokio::test]
async fn e020_12_02_retrieve_the_simplified_temporal_representation_of_an_entity_with_a_language_property_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created_vehicle(&broker, &namespace).await?;

    let id = namespace.urn("Vehicle", 1)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .temporal()
        .retrieve_values(&id)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": id.to_string(),
        "type": "Vehicle",
        "street": {
            "type": "LanguageProperty",
            "languageMaps": [
                [{"languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}}, "2020-08-01T12:03:00Z"],
                [{"languageMap": {"fr": "Grand Place", "nl": "Grote Markt", "es": "Gran Mercado"}}, "2020-08-01T12:05:00Z"],
                [
                    {"languageMap": {"fr": "Grand Marché", "nl": "Grote Markt", "es": "Gran Lugar", "it": "Grande Mercato"}},
                    "2020-08-01T12:07:00Z"
                ],
            ],
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));

    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
