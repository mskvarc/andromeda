//! ETSI `001_03`: a create whose identifier an entity already holds is refused with 409 `AlreadyExists`, and the entity
//! the broker holds is the one created first (clause 5.6.1).

use crate::{
    suite_keywords::{create_entity, delete_entities, ld_json_body_client, send_create_entity},
    suite_payloads::bare_building,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot create an entity with an existing id
/// Clause: 5.6.1
#[gtest]
#[tokio::test]
async fn e001_03_01_create_one_valid_entity_and_one_invalid_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let entity = bare_building(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &entity).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let answer = send_create_entity(&client, &entity).await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists).with_title());
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
