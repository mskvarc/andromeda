//! ETSI `013_02`: an attribute is not deleted when its entity id is absent or not a URI, or when its attribute name is
//! absent (clause 5.6.5).

use crate::{
    suite_keywords::{answered_with, create_vehicle_brand_parked_speed_gps, delete_entities},
    suite_payloads::VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    wire::{content_type, request_to},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::APPLICATION_JSON;

/// A `DELETE` of `target`, written as the ETSI keyword writes it and sent as `application/json` without a `Link`,
/// requiring `status`.
async fn refused_delete(broker: &BrokerClient, target: &str, status: StatusCode) -> Result<()> {
    answered_with(broker, content_type(request_to(broker, Method::DELETE, target)?, &APPLICATION_JSON)?, status).await
}

/// Pins: Check that one cannot delete an attribute from an entity with invalid/missing ids
/// Clause: 5.6.5
#[gtest]
#[tokio::test]
async fn e013_02_01_delete_an_attribute_if_the_entity_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;

    refused_delete(&broker, "entities//attrs/speed?deleteAll=false", StatusCode::BAD_REQUEST).await?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot delete an attribute from an entity with invalid/missing ids
/// Clause: 5.6.5
#[gtest]
#[tokio::test]
async fn e013_02_02_delete_an_attribute_if_the_entity_id_is_not_a_valid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;

    refused_delete(&broker, "entities/thisIsAnInvalidURI/attrs/speed?deleteAll=false", StatusCode::BAD_REQUEST).await?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot delete an attribute from an entity with invalid/missing ids
/// Clause: 5.6.5
#[gtest]
#[tokio::test]
async fn e013_02_03_delete_an_attribute_if_the_attribute_name_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;

    refused_delete(&broker, &format!("entities/{id}/attrs/?deleteAll=false"), StatusCode::METHOD_NOT_ALLOWED).await?;

    delete_entities(&broker, &[&id]).await;
    Ok(())
}
