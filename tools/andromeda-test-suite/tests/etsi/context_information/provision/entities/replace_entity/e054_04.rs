//! A replace whose attribute value is the NGSI-LD Null is refused with `BadRequestData` (clause 5.6.18).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, bare_building, building_simple_attributes},
    entity_operations::{create_entity, delete_entities},
    unchecked_requests::refused,
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, operation::ReplaceEntity},
    model::{ErrorType, Lossless, RawJson},
};

/// Pins: Check that replacing an entity with a payload containing an attribute whose value is NGSI-LD Null returns
/// 400 `BadRequestData`
/// Clause: 5.6.18
#[gtest]
#[tokio::test]
async fn e054_04_01_replace_entity_with_null_value_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let replacement = bare_building(&namespace, 1)?;
    let document = UncheckedDocument::from_entity(&replacement, Lossless::Normalized)
        .set_member("name", &r#"{"type": "Property", "value": "urn:ngsi-ld:null"}"#.parse::<RawJson>()?);
    let request = client.unchecked(&ReplaceEntity::builder().entity(&replacement).build())?.document(&document);
    refused(&client, request, ErrorType::BadRequestData).await?;
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
