//! ETSI `054_03`: a replace that names several scopes replaces the scope the entity had
//! (clauses 4.18, 5.6.18 and 6.5.3.3).

use crate::{
    suite_keywords::{create_entity_with_link, delete_entities, retrieve_entity_json},
    suite_payloads::{building_with_many_scopes, building_with_one_scope},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that one can replace an existing entity and that scopes are replaced
/// Clause: 4.18, 5.6.18, 6.5.3.3
#[gtest]
#[tokio::test]
async fn e054_03_01_replace_an_existing_entity_having_scopes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let original = building_with_one_scope(&namespace)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    let client = broker.with_context(vocabulary.context())?;
    let replacement = building_with_many_scopes(&namespace)?;
    let replaced = client.with_responses().entities().replace(&replacement).send().await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))?;
    expect_that!(replaced?.body.len(), eq(0));
    let body = retrieve_entity_json(&client.with_accept(NgsiLdMediaType::Json), &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "scope": ["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"],
        }))
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
