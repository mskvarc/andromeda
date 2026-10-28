//! ETSI `056_06`: a Merge Entity whose fragment gives a `datasetId` of the NGSI-LD Null is refused with 400
//! `BadRequestData` (clauses 5.6.17 and 6.5.3.4).

use crate::{
    suite_keywords::{create_entity, delete_entities},
    suite_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::ContextDelivery,
    model::{DatasetId, EntityFragment, ErrorType, Property},
};

/// Pins: Check that a Merge Entity operation setting an NGSI-LD Null datasetId returns 400 `BadRequestData`
/// Clause: 5.6.17
#[gtest]
#[tokio::test]
async fn e056_06_01_merge_entity_setting_dataset_id_to_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .property("name", Property::builder("Eiffel Tower").dataset_id("urn:ngsi-ld:null".parse::<DatasetId>()?))
        .build()?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = client.with_responses().entities().merge(&original.id, &fragment).send().await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title())?;
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
