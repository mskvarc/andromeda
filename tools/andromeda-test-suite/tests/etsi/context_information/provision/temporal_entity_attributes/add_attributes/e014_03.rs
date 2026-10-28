//! ETSI `014_03`: instances are not added to a temporal representation of an entity that does not exist (clause 5.6.12).

use crate::{suite_keywords::ld_json_body_client, suite_payloads::added_attribute_instances};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: Check that an error is raised if one adds an attribute to a non-existent entity
/// Clause: 5.6.12
#[gtest]
#[tokio::test]
async fn e014_03_01_add_attribute_to_temporal_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["added_attribute"]).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let answer = client
        .with_responses()
        .temporal()
        .add_attributes(&id, &added_attribute_instances()?)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    Ok(())
}
