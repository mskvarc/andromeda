//! An update is refused with 404 `ResourceNotFound` when its entity id is not known to the system (clause 5.6.2).

use crate::{
    entity_operations::{delete_entities, ld_json_body_client},
    vehicle_entity_payloads::speedometer_speed_99_fragment,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// The terms of the `@context` the case registers: `speed` and `source`.
const TERMS: [&str; 2] = ["speed", "source"];

/// Pins: Check that one cannot update entity attributes if the entity id or attributes are not known to the system
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_03_01_update_entity_attributes_when_the_entity_id_is_not_known_to_the_system() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let fragment = speedometer_speed_99_fragment()?;
    let answer = client.with_responses().entities().update_attributes(&id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
