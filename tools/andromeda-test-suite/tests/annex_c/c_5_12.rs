//! Annex C.5.12: Retrieve Available Attribute Information answers the attribute types, the types using the
//! attribute and its count (clause 5.7.10).

use crate::{discovery_fleet::fleet_tenant, served_context::ok_body};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;
use ngsi_ld::model::{AttributeName, NgsiLdMediaType};
use serde_json::json;

/// Pins: Retrieve Available Attribute Information for brandName answers its attribute types, the types using it and
/// its count as C.5.12.3 prints
/// Clause: C.5.12, 4.5.15, 5.7.10
/// Tenant: private (type and attribute discovery answers every type and attribute of the tenant, 5.7.5 to 5.7.10,
/// which only a tenant holding exactly the four Entities this file creates can show)
#[gtest]
#[tokio::test]
async fn c_5_12_0001_the_attribute_information_of_brand_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, tenant) = fleet_tenant(&namespace).await?;
    let brand: AttributeName = "brandName".parse()?;
    let body = ok_body(
        tenant
            .with_accept(NgsiLdMediaType::Json)
            .with_responses()
            .attributes()
            .retrieve(&brand)
            .send()
            .await,
    )?;
    expect_that!(
        &body,
        eq(&json!({
            "id": "http://example.org/vehicle/brandName",
            "type": "Attribute",
            "attributeName": "brandName",
            "attributeTypes": ["Property"],
            "typeNames": [namespace.suffixed("Vehicle")],
            "attributeCount": 2,
        }))
    );
    served.stop();
    Ok(())
}
