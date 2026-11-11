//! The entity type lists use the declared terms.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Context,
        Entity,
        EntityTypeName,
        NgsiLdMediaType,
        Uri,
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
    let details = client.entity_types().list_details().send().await?;
    let vehicle_id: Uri = format!("http://example.org/{namespace}/Vehicle").parse()?;
    let vehicle = details.iter().find(|item| item.id == vehicle_id).or_fail()?;
    expect_that!(vehicle.type_name, eq(&vehicle_term));
    expect_that!(
        vehicle.attribute_names,
        unordered_elements_are![
            eq(&format!("isParked_{namespace}").parse::<AttributeName>()?),
            eq(&format!("speed_{namespace}").parse::<AttributeName>()?),
            eq(&"location".parse::<AttributeName>()?),
            eq(&format!("http://example.org/{namespace}/status").parse::<AttributeName>()?)
        ]
    );
    Ok(())
}
