//! ETSI `037_08`: a query of context source registrations without a temporal query answers the registrations of
//! latest information, trimmed to the matching `RegistrationInfo` (clause 5.10.2).

use crate::{
    suite_keywords::{create_registration, csource_endpoint, delete_registrations},
    suite_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use serde_json::json;

/// Pins: Check that one can query context source registrations. If no temporal query is present, only Context Source
/// Registrations for Context Sources providing latest information are considered
/// Clause: 5.10.2
/// Corrected: fix(csr-discovery-trim): trim discovery coverage to matching registrationinfo (5.10.2.5)
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_08_01_query_context_source_registration_without_temporal_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = vehicle_parking_registration(&namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let _registration = create_registration(&broker, &vocabulary, &registration).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("ContextSourceRegistration", 1),
            "type": "ContextSourceRegistration",
            "information": [{
                "entities": [{"id": namespace.urn_text("Vehicle", "A456"), "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            }],
            "endpoint": endpoint.as_str(),
        }]))
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    delete_registrations(&broker, &namespace, &[1]).await
}
