//! Annex C.2.4: one merged `@context` holding the terms of the three vocabulary documents renders what the three
//! render (clause 4.4).

use crate::{
    printed_document::expect_with_any_context,
    served_context::{ServedContexts, linked, ok_body},
    vehicle_example::{aggregated_context, vehicle},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::{Value, json};
use url::Url;

/// Serves the merged `@context` the annex prints and answers its URL.
async fn merged_context(served: &ServedContexts, namespace: &Namespace) -> Result<Url> {
    let vehicle = namespace.suffixed("Vehicle");
    let parking = namespace.suffixed("OffStreetParking");
    let document: Value = json!({"@context": {
        "id": "@id",
        "type": "@type",
        "Property": "https://uri.etsi.org/ngsi-ld/Property",
        "Relationship": "https://uri.etsi.org/ngsi-ld/Relationship",
        "value": "https://uri.etsi.org/ngsi-ld/hasValue",
        "object": {"@type": "@id", "@id": "https://uri.etsi.org/ngsi-ld/hasObject"},
        "observedAt": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "https://uri.etsi.org/ngsi-ld/observedAt"},
        "datasetId": {"@id": "https://uri.etsi.org/ngsi-ld/datasetId", "@type": "@id"},
        "location": "https://uri.etsi.org/ngsi-ld/location",
        "GeoProperty": "https://uri.etsi.org/ngsi-ld/GeoProperty",
        vehicle.as_str(): format!("http://example.org/vehicle/{vehicle}"),
        "street": "http://example.org/vehicle/street",
        "brandName": "http://example.org/vehicle/brandName",
        "category": "http://example.org/vehicle/category",
        "tyreTreadDepths": "http://example.org/vehicle/tyreTreadDepths",
        "passengers": "http://example.org/vehicle/passengers",
        "speed": "http://example.org/vehicle/speed",
        "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"},
        parking.as_str(): format!("http://example.org/parking/{parking}"),
        "availableSpotNumber": "http://example.org/parking/availableSpotNumber",
        "totalSpotNumber": "http://example.org/parking/totalSpotNumber",
        "isNextToBuilding": {"@type": "@id", "@id": "http://example.org/common/isNextToBuilding"},
        "reliability": "http://example.org/common/reliability",
        "providedBy": {"@type": "@id", "@id": "http://example.org/common/providedBy"},
        "name": "http://example.org/common/name",
        "commercial": "http://example.org/vehicle/commercial",
        "non-commercial": "http://example.org/vehicle/non-commercial",
        "Integer": "http://www.w3.org/2001/XMLSchema#Integer",
    }});
    served.serve("merged-context.jsonld", &document).await
}

/// Pins: A Vehicle written under the three separate vocabulary documents reads back under the one merged @context
/// C.2.4 prints with the same short names
/// Clause: C.2.4, 4.4, 5.5.7, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_4_0001_the_merged_context_renders_what_its_parts_render() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let front = broker::front()?;
    let writer = linked(&front, &aggregated_context(&served, &namespace).await?)?;
    let reader = linked(&front, &merged_context(&served, &namespace).await?)?;
    let vehicle = vehicle(&namespace, None)?;
    writer.entities().create(&vehicle).send().await?;
    let body = ok_body(
        reader
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve(&vehicle.id)
            .attrs(vec!["brandName".parse()?, "isParked".parse()?])
            .send()
            .await,
    )?;
    let expected = json!({
        "id": vehicle.id.to_string(),
        "type": namespace.suffixed("Vehicle"),
        "brandName": {"type": "Property", "value": "Mercedes"},
        "isParked": {
            "type": "Relationship",
            "objectType": namespace.suffixed("OffStreetParking"),
            "object": namespace.urn_text("OffStreetParking", "Downtown1"),
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
        },
    });
    expect_with_any_context(body, &expected);
    served.stop();
    Ok(())
}
