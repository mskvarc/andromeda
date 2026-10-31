//! A relationship object is found by the uri that created it.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
    query::{Locality, q},
};

/// Pins: Query Entities with q isParkedIn=="HTTP://Example.com/a" finds the Entity whose Relationship `isParkedIn`
/// was created with that object
/// Clause: 4.9, 5.5.1, 5.7.2; IETF RFC 3986 section 6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9046_a_relationship_object_is_found_by_the_uri_that_created_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let uri_comparison_as_written_id = namespace.urn("UriComparison", "as-written")?;
    let uri_comparison_type = namespace.entity_type("UriComparison")?;
    let uri_comparison_as_written = Entity::builder()
        .id(uri_comparison_as_written_id.clone())
        .entity_type(uri_comparison_type.clone())
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.isParkedIn.object (written "HTTP://Example.com/a")]: {"id":"urn:ngsi-ld:UriComparison:zqns-as-written","type":"UriComparison_zqns","isParkedIn":{"type":"Relationship","object":"http://example.com/a"}}), so `isParkedIn` is written as text.
    let is_parked_in: RawJson = r#"{"type": "Relationship", "object": "HTTP://Example.com/a"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&uri_comparison_as_written, Lossless::Normalized).set_member("isParkedIn", &is_parked_in);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&uri_comparison_as_written).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&uri_comparison_type)
        .locality(Locality::Local)
        .ids(vec![uri_comparison_as_written_id.clone()])
        .q(q::attr("isParkedIn").eq("HTTP://Example.com/a"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&uri_comparison_as_written_id)));
    Ok(())
}
