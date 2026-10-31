//! An appended attribute whose value is the NGSI-LD Null is refused with 400 `BadRequestData` (clause 5.6.3).

use crate::entity_operations::{create_entity, delete_entities, ld_json_body_client};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::AppendAttributes},
    model::{Entity, EntityFragment, ErrorType, Input, RawJson},
};

/// The terms of the `@context` the case registers.
const TERMS: [&str; 2] = ["Building", "name"];

/// Pins: Check that appending an attribute whose value is NGSI-LD Null returns 400 `BadRequestData`
/// Clause: 5.6.3
#[gtest]
#[tokio::test]
async fn e010_12_01_append_attribute_with_null_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original: Entity<Input> = Entity::builder().id(namespace.urn("Vehicle", 1)?).entity_type("Building").build()?;
    create_entity(&broker, &vocabulary, &original).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let valid = EntityFragment::builder().property("name", "Eiffel Tower").build()?;
    let operation = AppendAttributes::builder().id(&original.id).fragment(&valid).build();
    let null_name: RawJson = r#"{"type":"Property","value":"urn:ngsi-ld:null"}"#.parse()?;
    let document = UncheckedDocument::empty().set_member("name", &null_name);
    let request = client.unchecked(&operation)?.document(&document);
    let answer = client.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
