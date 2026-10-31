//! A context source registration sent without an id is created with the id the broker names in its `Location` (clause
//! 5.9.2).

use crate::{
    registration_operations::csource_endpoint,
    registration_payloads::{VEHICLE_PARKING_TERMS, a456_information, offstreet_pattern, parking_information},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::{header_value, json_body},
};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Input, NgsiLdMediaType},
    registration::CSourceRegistration,
};
use serde_json::json;
use url::Url;

/// The registration of the case, sent without an id: the Vehicle `urn:ngsi-ld:Vehicle:<ns>-A456` and the
/// `OffStreetParking` patterns, at `endpoint`.
fn registration(namespace: &Namespace, endpoint: &Url) -> Result<CSourceRegistration<Input>> {
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .information([a456_information(namespace.urn("Vehicle", "A456")?)?, parking_information(namespace)?])
        .build()?)
}

/// Pins: Check that one can create a context source registration without specifying an ID
/// Clause: 5.9.2
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e033_11_create_context_source_registration_without_specifying_an_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let client: BrokerClient = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let _registration = Cleanup::of(&client).registration_of(&registration(&namespace, &endpoint)?);
    let created = client
        .with_responses()
        .registrations()
        .register(&registration(&namespace, &endpoint)?)
        .send()
        .await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    let received = created?;
    expect_that!(received.body.len(), eq(0));
    let id = received.outcome.id().or_fail()?;
    expect_that!(id.as_str(), not(eq("")));
    let location = header_value(received.head.headers(), &LOCATION).or_fail()?;
    let id_suffix = format!("/{id}");
    expect_that!(location, ends_with(id_suffix.as_str()));

    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .with_responses()
        .registrations()
        .retrieve(&id)
        .send()
        .await;
    let body = json_body(&retrieved?)?;
    let downtown = offstreet_pattern(&namespace, ".*downtown$")?;
    let forty_seven = offstreet_pattern(&namespace, ".*47$")?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "ContextSourceRegistration",
            "information": [
                {
                    "entities": [{"id": namespace.urn_text("Vehicle", "A456"), "type": "Vehicle"}],
                    "propertyNames": ["brandName", "speed"],
                    "relationshipNames": ["isParked"]
                },
                {
                    "entities": [
                        {"idPattern": downtown.as_ref(), "type": "OffStreetParking"},
                        {"idPattern": forty_seven.as_ref(), "type": "OffStreetParking"}
                    ],
                    "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                    "relationshipNames": ["isNextToBuilding"]
                }
            ],
            "endpoint": endpoint.as_str(),
        }))
        .hiding_everywhere("@context")
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );

    broker.registrations().delete(&id).send().await?;
    Ok(())
}
