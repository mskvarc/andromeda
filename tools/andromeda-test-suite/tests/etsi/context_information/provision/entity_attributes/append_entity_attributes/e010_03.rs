//! ETSI `010_03`: an append to an entity id the system does not hold is refused with 404 `ResourceNotFound`
//! (clause 5.6.3).

use crate::{
    suite_keywords::{delete_entities, ld_json_body_client},
    suite_payloads::speed_and_added_attribute,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// The terms of the `@context` the case registers: `speed`, `source` and the appended `attribute_to_be_added`.
const TERMS: [&str; 3] = ["speed", "source", "attribute_to_be_added"];

/// Pins: Check that one cannot append entity attributes if the entity id is not known to the system
/// Clause: 5.6.3
#[gtest]
#[tokio::test]
async fn e010_03_01_append_entity_attributes_when_the_entity_id_is_not_known_to_the_system() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let fragment = speed_and_added_attribute(&namespace, "speedometer")?;
    let answer = client.with_responses().entities().append_attributes(&id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
