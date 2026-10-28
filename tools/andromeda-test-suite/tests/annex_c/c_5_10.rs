//! Annex C.5.10: Retrieve Available Attributes names the attributes the supplied `@context` knows by short name and
//! the others by Fully Qualified Name (clause 5.7.8).

use crate::{discovery_fleet::fleet_tenant, printed_document::sort_texts, served_context::ok_body};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Retrieve Available Attributes answers an `AttributeList` naming the attributes the supplied @context knows
/// by short name and the rest by Fully Qualified Name, as C.5.10.3 prints
/// Clause: C.5.10, 4.5.13, 5.7.8
/// Corrected: complete-attribute-lists: the lists hold the `OffStreetParking` Attributes C.5.8 shows on the same
/// Entities (5.7.8, 5.7.9)
/// Tenant: private (type and attribute discovery answers every type and attribute of the tenant, 5.7.5 to 5.7.10,
/// which only a tenant holding exactly the four Entities this file creates can show)
#[gtest]
#[tokio::test]
async fn c_5_10_0001_the_attribute_list_names_known_attributes_short() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, tenant) = fleet_tenant(&namespace).await?;
    let mut body = ok_body(tenant.with_accept(NgsiLdMediaType::Json).with_responses().attributes().list().send().await)?;
    expect_that!(
        body.get("id").and_then(serde_json::Value::as_str),
        some(matches_regex("^urn:ngsi-ld:AttributeList:.*"))
    );
    if let Some(members) = body.as_object_mut() {
        members.remove("id");
    }
    // Clause 5.7.8 leaves the order of the attribute list open.
    sort_texts(&mut body, "/attributeList");
    expect_that!(
        &body,
        eq(&json!({
            "type": "AttributeList",
            "attributeList": [
                "availableSpotNumber",
                "brandName",
                "http://example.org/parking/status",
                "isNextToBuilding",
                "isParked",
                "location",
                "speed",
                "totalSpotNumber",
            ],
        }))
    );
    served.stop();
    Ok(())
}
