//! ETSI `024_01`: an entity type that no entity of the tenant holds is not retrieved in its detailed representation
//! when the request names it without a `@context` (clause 5.7.7).

use crate::{suite_keywords::create_entity_with_link, suite_payloads::building_simple_attributes};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{EntityTypeName, ErrorType};

/// The terms of the case's `@context`.
const TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// Pins: Check that one cannot retrieve a detailed representation of an unknown NGSI-LD entity type
/// Clause: 5.7.7
/// Tenant: private (a type or attribute without a @context expands under the default context, which any other file
/// may also use)
#[gtest]
#[tokio::test]
async fn e024_01_01_retrieve_detailed_representation_of_available_entity_type_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &TERMS).await?;
    let building = building_simple_attributes(&namespace, 1)?;
    let second_building = building_simple_attributes(&namespace, 2)?;
    create_entity_with_link(&tenant, &vocabulary, &building).await?;
    create_entity_with_link(&tenant, &vocabulary, &second_building).await?;
    let building_type: EntityTypeName = "Building".parse()?;
    let answer = tenant.with_responses().entity_types().retrieve(&building_type).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    tenant.entities().delete(&building.id).send().await.any_answer();
    tenant.temporal().delete(&building.id).send().await.any_answer();
    tenant.entities().delete(&second_building.id).send().await.any_answer();
    tenant.temporal().delete(&second_building.id).send().await.any_answer();
    Ok(())
}
