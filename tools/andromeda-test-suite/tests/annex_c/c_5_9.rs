//! Annex C.5.9: Retrieve Available Entity Type Information answers the entity count of a type and the details of
//! its attributes (clause 5.7.7).

use crate::{
    discovery_fleet::fleet_tenant,
    printed_document::{sort_items_by, sort_list_by},
    served_context::ok_body,
};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::{Value, json};

/// The attribute detail `id` named `name` of the one kind `kind`.
fn detail(id: &str, name: &str, kind: &str) -> Value {
    json!({"id": id, "type": "Attribute", "attributeName": name, "attributeTypes": [kind]})
}

/// Pins: Retrieve Available Entity Type Information for Vehicle answers its entity count and the details of its
/// attributes, in any order, as C.5.9.3 prints
/// Clause: C.5.9, 4.5.12, 5.7.7
/// Tenant: private (type and attribute discovery answers every type and attribute of the tenant, 5.7.5 to 5.7.10,
/// which only a tenant holding exactly the four Entities this file creates can show)
#[gtest]
#[tokio::test]
async fn c_5_9_0001_the_entity_type_information_of_vehicle() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, tenant) = fleet_tenant(&namespace).await?;
    let vehicle = namespace.entity_type("Vehicle")?;
    let mut body = ok_body(
        tenant
            .with_accept(NgsiLdMediaType::Json)
            .with_responses()
            .entity_types()
            .retrieve(&vehicle)
            .send()
            .await,
    )?;
    // Clause 5.7.7 leaves the order of the attribute details open.
    sort_list_by(&mut body, "/attributeDetails", "id");
    let mut details = vec![
        detail("http://example.org/vehicle/brandName", "brandName", "Property"),
        detail("http://example.org/common/isParked", "isParked", "Relationship"),
        detail("https://uri.etsi.org/ngsi-ld/location", "location", "GeoProperty"),
        detail("http://example.org/vehicle/speed", "speed", "Property"),
    ];
    sort_items_by(&mut details, "id");
    expect_that!(
        &body,
        eq(&json!({
            "id": format!("http://example.org/vehicle/{}", namespace.suffixed("Vehicle")),
            "type": "EntityTypeInfo",
            "typeName": namespace.suffixed("Vehicle"),
            "entityCount": 2,
            "attributeDetails": details,
        }))
    );
    served.stop();
    Ok(())
}
