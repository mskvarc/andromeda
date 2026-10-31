//! The entity type lists use the declared terms.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_includes},
    broker,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RetrieveEntityTypeDetails},
    model::{
        Context,
        Entity,
        EntityTypeName,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Available Entity Types under a declaring @context answers an `EntityTypeList` naming the declared
/// type term, and with details=true a record whose id is the type's full IRI, whose `typeName` is the term and
/// whose `attributeNames` are the declared terms, location and the undeclared attribute's full IRI
/// Clause: 5.7.5, 5.7.6, Table 5.2.25-1
#[gtest]
#[tokio::test]
async fn n_dsc_tyd_9001_the_entity_type_lists_use_the_declared_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/declaring.jsonld")?))?;
    let vehicle_term = namespace.entity_type("Vehicle")?;
    contexts
        .serve(
            "ctx/declaring.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Vehicle_{namespace}": "http://example.org/{namespace}/Vehicle",
        "speed_{namespace}": "http://example.org/{namespace}/speed",
        "isParked_{namespace}": {{
          "@type": "@id",
          "@id": "http://example.org/{namespace}/isParked"
        }}
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(format!("http://example.org/{namespace}/Vehicle").parse::<EntityTypeName>()?)
        .property(format!("http://example.org/{namespace}/speed").as_str(), 55)
        .relationship(format!("http://example.org/{namespace}/isParked").as_str(), off_street_parking_downtown1_id)
        .property(format!("http://example.org/{namespace}/status").as_str(), "free")
        .geo_property("location", Shape::Point(Position::try_new(-8.5, 41.2)?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let client = linked.with_accept(Accept::new(NgsiLdMediaType::Json));
    let types = client.with_heads().entity_types().list().send().await?;
    expect_that!(types.status, eq(StatusCode::OK));
    expect_that!(types.outcome.type_list, contains(eq(&vehicle_term)));
    // Sibling tests leave the reserved `ngsildproof` attribute in the tenant, which the typed list refuses, so the catalog is read as received.
    let details = client.send_unchecked(client.unchecked(&RetrieveEntityTypeDetails::builder().build())?).await?;
    expect_that!(details.status(), eq(StatusCode::OK));
    let vehicle_details = format!("$[?@.id == 'http://example.org/{namespace}/Vehicle']");
    expect_that!(details.body(), json_equals(&format!("{vehicle_details}.typeName"), vehicle_term.to_string()));
    expect_that!(details.body(), json_count(&format!("{vehicle_details}.attributeNames"), 4));
    for name in [
        format!("isParked_{namespace}"),
        format!("speed_{namespace}"),
        "location".to_owned(),
        format!("http://example.org/{namespace}/status"),
    ] {
        expect_that!(details.body(), json_includes(&format!("{vehicle_details}.attributeNames"), name));
    }
    Ok(())
}
