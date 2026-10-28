//! ETSI `001_02`: a create whose body is not a valid entity is refused with a problem of the type its defect calls for
//! (clause 5.6.1).

use crate::suite_keywords::{LD_JSON, post_text, refused};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot create an entity with an invalid request
/// Clause: 5.6.1
#[gtest]
#[tokio::test]
async fn e001_02_01_invalid_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let text = format!(
        "[\n   {{\n      \"id\":\"urn:ngsi-ld:Building:{namespace}-randomUUID\",,\n      \"type\":\"Building\",\n      \"@context\":[\n         \"{}\",\n         \"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld\"\n      ]\n   }}\n]",
        vocabulary.url().as_str()
    );
    let answer = broker.send_unchecked(post_text(&broker, "entities/", LD_JSON, &text)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    Ok(())
}

/// Pins: Check that one cannot create an entity with an invalid request
/// Clause: 5.6.1
/// Corrected: `fix(001_02_02)`: empty json array is badrequestdata not invalidrequest (5.5.4)
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e001_02_02_empty_json() -> Result<()> {
    let broker = broker::front()?;
    refused(&broker, post_text(&broker, "entities/", LD_JSON, "[]")?, ErrorType::BadRequestData).await
}

/// Pins: Check that one cannot create an entity with an invalid request
/// Clause: 5.6.1
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e001_02_03_entity_with_no_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let text = format!("{{\n    \"id\": \"urn:ngsi-ld:Building:{namespace}-randomUUID\",\n    \"type\": \"Building\"\n}}");
    refused(&broker, post_text(&broker, "entities/", LD_JSON, &text)?, ErrorType::BadRequestData).await
}

/// Pins: Check that one cannot create an entity with an invalid request
/// Clause: 5.6.1
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e001_02_04_entity_with_invalid_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let text = format!(
        "{{\n  \"id\":\"urn:ngsi-ld:Building:{namespace}-randomUUID\",\n  \"type\":\"type\",\n  \"@context\":[\n    \"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld\"\n  ]\n}}\n"
    );
    refused(&broker, post_text(&broker, "entities/", LD_JSON, &text)?, ErrorType::BadRequestData).await
}

/// Pins: Check that one cannot create an entity with an invalid request
/// Clause: 5.6.1
#[gtest]
#[tokio::test]
async fn e001_02_05_entity_with_null_value_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name"]).await?;
    let text = format!(
        "{{\n  \"id\": \"urn:ngsi-ld:Building:{namespace}-randomUUID\",\n  \"type\": \"Building\",\n  \"name\": {{\n    \"type\": \"Property\",\n    \"value\": \"urn:ngsi-ld:null\"\n  }},\n  \"@context\": \"{}\"\n}}\n",
        vocabulary.url().as_str()
    );
    refused(&broker, post_text(&broker, "entities/", LD_JSON, &text)?, ErrorType::BadRequestData).await
}
