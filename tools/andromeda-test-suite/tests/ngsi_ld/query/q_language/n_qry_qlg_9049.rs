//! A urn object matches with any case prefix and only its own case specific string.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Query Entities with q `isParkedIn` equal to `urn:ngsi-ld:OffStreetParking:<ns>-Downtown1` or
/// `URN:NGSI-LD:OffStreetParking:<ns>-Downtown1` finds the Entity whose `isParkedIn` object is
/// `urn:ngsi-ld:OffStreetParking:<ns>-Downtown1`, and urn:ngsi-ld:offstreetparking:<ns>-downtown1 finds nothing
/// Clause: 4.9 (EXAMPLE 3), 5.5.1, 5.7.2; IETF RFC 3986 sections 6.2.2.1 and 6.2.3; IETF RFC 8141 section 3.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9049_a_urn_object_matches_with_any_case_prefix_and_only_its_own_case_specific_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let uri_comparison_urn_object_id = namespace.urn("UriComparison", "urn-object")?;
    let uri_comparison_type = namespace.entity_type("UriComparison")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let uri_comparison_urn_object = Entity::builder()
        .id(uri_comparison_urn_object_id.clone())
        .entity_type(uri_comparison_type.clone())
        .relationship("isParkedIn", off_street_parking_downtown1_id)
        .build()?;
    broker.entities().create(&uri_comparison_urn_object).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_urn_object_id.clone()])
        .q(q::attr("isParkedIn").eq(format!("urn:ngsi-ld:OffStreetParking:{namespace}-Downtown1").as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&uri_comparison_urn_object_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_urn_object_id.clone()])
        .q(q::attr("isParkedIn").eq(format!("URN:NGSI-LD:OffStreetParking:{namespace}-Downtown1").as_str()))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&uri_comparison_urn_object_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_urn_object_id.clone()])
        .q(q::attr("isParkedIn").eq(format!("urn:ngsi-ld:offstreetparking:{namespace}-downtown1").as_str()))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
