//! Annex C.5.7: Retrieve Available Entity Types names the types the supplied `@context` knows by short name and the
//! others by Fully Qualified Name (clause 5.7.5).

use crate::{discovery_fleet::fleet_tenant, printed_document::sort_texts, served_context::ok_body};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Retrieve Available Entity Types answers an `EntityTypeList` naming the types the supplied @context knows by
/// short name and the rest by Fully Qualified Name, as C.5.7.3 prints
/// Clause: C.5.7, 4.5.10, 5.7.5
/// Tenant: private (type and attribute discovery answers every type and attribute of the tenant, 5.7.5 to 5.7.10,
/// which only a tenant holding exactly the four Entities this file creates can show)
#[gtest]
#[tokio::test]
async fn c_5_7_0001_the_entity_type_list_names_known_types_short() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, tenant) = fleet_tenant(&namespace).await?;
    let mut body = ok_body(tenant.with_accept(NgsiLdMediaType::Json).with_responses().entity_types().list().send().await)?;
    expect_that!(
        body.get("id").and_then(serde_json::Value::as_str),
        some(matches_regex("^urn:ngsi-ld:EntityTypeList:.*"))
    );
    if let Some(members) = body.as_object_mut() {
        members.remove("id");
    }
    // Clause 5.7.5 leaves the order of the type list open.
    sort_texts(&mut body, "/typeList");
    let mut types = vec![
        namespace.suffixed("OffStreetParking"),
        namespace.suffixed("Vehicle"),
        format!("http://example.org/parking/{}", namespace.suffixed("ParkingSpot")),
    ];
    types.sort();
    expect_that!(&body, eq(&json!({"type": "EntityTypeList", "typeList": types})));
    served.stop();
    Ok(())
}
