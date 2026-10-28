//! Annex C.5.8: Retrieve Details of Available Entity Types answers one `EntityType` per type with its attribute
//! names (clause 5.7.6).

use crate::{
    discovery_fleet::fleet_tenant,
    printed_document::{sort_items_by, sort_texts},
    served_context::ok_body,
};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::{Value, json};

/// The `EntityType` `id` named `name`, its attribute names `attributes` in text order.
fn entity_type(id: &str, name: &str, attributes: &[&str]) -> Value {
    json!({"id": id, "type": "EntityType", "typeName": name, "attributeNames": attributes})
}

/// Pins: Retrieve Details of Available Entity Types answers one `EntityType` per type with its attribute names, in
/// any order, as C.5.8.3 prints
/// Clause: C.5.8, 4.5.11, 5.7.6
/// Tenant: private (type and attribute discovery answers every type and attribute of the tenant, 5.7.5 to 5.7.10,
/// which only a tenant holding exactly the four Entities this file creates can show)
#[gtest]
#[tokio::test]
async fn c_5_8_0001_the_detailed_entity_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, tenant) = fleet_tenant(&namespace).await?;
    let body = ok_body(
        tenant
            .with_accept(NgsiLdMediaType::Json)
            .with_responses()
            .entity_types()
            .list_details()
            .send()
            .await,
    )?;
    // Clause 5.7.6 leaves the order of the types and of their attribute names open.
    let Value::Array(mut types) = body else {
        return fail!("the detailed entity types are not an array: {body}");
    };
    sort_items_by(&mut types, "id");
    for entity_type in &mut types {
        sort_texts(entity_type, "/attributeNames");
    }
    let spot = format!("http://example.org/parking/{}", namespace.suffixed("ParkingSpot"));
    let mut expected = vec![
        entity_type(
            &format!("http://example.org/vehicle/{}", namespace.suffixed("Vehicle")),
            &namespace.suffixed("Vehicle"),
            &["brandName", "isParked", "location", "speed"],
        ),
        entity_type(
            &format!("http://example.org/parking/{}", namespace.suffixed("OffStreetParking")),
            &namespace.suffixed("OffStreetParking"),
            &["availableSpotNumber", "isNextToBuilding", "location", "totalSpotNumber"],
        ),
        entity_type(&spot, &spot, &["http://example.org/parking/status", "location"]),
    ];
    sort_items_by(&mut expected, "id");
    expect_that!(&types, eq(&expected));
    served.stop();
    Ok(())
}
