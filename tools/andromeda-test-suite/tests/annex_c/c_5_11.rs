//! Annex C.5.11: Retrieve Details of Available Attributes answers one Attribute per attribute with the types using
//! it (clause 5.7.9).

use crate::{
    discovery_fleet::fleet_tenant,
    printed_document::{sort_items_by, sort_texts},
    served_context::ok_body,
};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::{Value, json};

/// The Attribute `id` named `name`, used by the types `types` in text order.
fn attribute(id: &str, name: &str, types: &[String]) -> Value {
    json!({"id": id, "type": "Attribute", "attributeName": name, "typeNames": types})
}

/// Pins: Retrieve Details of Available Attributes answers one Attribute per attribute with the types using it, in any
/// order, as C.5.11.3 prints
/// Clause: C.5.11, 4.5.14, 5.7.9
/// Corrected: complete-attribute-lists: the lists hold the `OffStreetParking` Attributes C.5.8 shows on the same
/// Entities (5.7.8, 5.7.9)
/// Tenant: private (type and attribute discovery answers every type and attribute of the tenant, 5.7.5 to 5.7.10,
/// which only a tenant holding exactly the four Entities this file creates can show)
#[gtest]
#[tokio::test]
async fn c_5_11_0001_the_detailed_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, tenant) = fleet_tenant(&namespace).await?;
    let body = ok_body(
        tenant
            .with_accept(NgsiLdMediaType::Json)
            .with_responses()
            .attributes()
            .list_details()
            .send()
            .await,
    )?;
    // Clause 5.7.9 leaves the order of the attributes and of their type names open.
    let Value::Array(mut attributes) = body else {
        return fail!("the detailed attributes are not an array: {body}");
    };
    sort_items_by(&mut attributes, "id");
    for attribute in &mut attributes {
        sort_texts(attribute, "/typeNames");
    }
    let parking = [namespace.suffixed("OffStreetParking")];
    let vehicle = [namespace.suffixed("Vehicle")];
    let spot_type = || format!("http://example.org/parking/{}", namespace.suffixed("ParkingSpot"));
    let spot = [spot_type()];
    let mut located = vec![namespace.suffixed("OffStreetParking"), namespace.suffixed("Vehicle"), spot_type()];
    located.sort();
    let mut expected = vec![
        attribute("http://example.org/parking/availableSpotNumber", "availableSpotNumber", &parking),
        attribute("http://example.org/vehicle/brandName", "brandName", &vehicle),
        attribute("http://example.org/common/isNextToBuilding", "isNextToBuilding", &parking),
        attribute("http://example.org/common/isParked", "isParked", &vehicle),
        attribute("https://uri.etsi.org/ngsi-ld/location", "location", &located),
        attribute("http://example.org/vehicle/speed", "speed", &vehicle),
        attribute("http://example.org/parking/status", "http://example.org/parking/status", &spot),
        attribute("http://example.org/parking/totalSpotNumber", "totalSpotNumber", &parking),
    ];
    sort_items_by(&mut expected, "id");
    expect_that!(&attributes, eq(&expected));
    served.stop();
    Ok(())
}
