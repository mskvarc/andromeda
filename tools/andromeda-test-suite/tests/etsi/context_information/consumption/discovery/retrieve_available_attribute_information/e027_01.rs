//! An attribute that the request names without a `@context` is not known to the broker, so its detailed representation
//! is not retrieved (clause 5.7.10).

use crate::{building_payloads::building_simple_attributes, entity_operations::create_entity_with_link};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, ErrorType};

/// The terms of the case's `@context`.
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// Pins: Check that one cannot retrieve a detailed representation of an unknown NGSI-LD attribute
/// Clause: 5.7.10
/// Tenant: private (a type or attribute without a @context expands under the default context, which any other file
/// may also use)
#[gtest]
#[tokio::test]
async fn e027_01_01_retrieve_detailed_representation_of_available_attribute_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&tenant, &vocabulary, &building).await?;
    let attribute: AttributeName = "airQualityLevel".parse()?;
    let answer = tenant.with_responses().attributes().retrieve(&attribute).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
