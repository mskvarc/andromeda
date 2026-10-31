//! The entity type information pairs short names with full uris.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeKind,
        AttributeName,
        Context,
        Entity,
        EntityTypeName,
        NgsiLdMediaType,
        Uri,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity Type Information of the declared type term answers an `EntityTypeInfo` whose id is the
/// type's full IRI, whose `typeName` is the term and whose `entityCount` is 1, with `attributeDetails` naming the
/// declared Relationship by its term, location by its name beside its full URI, and the undeclared attribute by its
/// full IRI
/// Clause: 5.7.7, Table 5.2.26-1, Table 5.2.28-1
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_9001_the_entity_type_information_pairs_short_names_with_full_uris() -> Result<()> {
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
    let information = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_types()
        .retrieve(&vehicle_term)
        .send()
        .await?;
    let vehicle_iri: Uri = format!("http://example.org/{namespace}/Vehicle").parse()?;
    expect_that!(information.id, eq(&vehicle_iri));
    expect_that!(information.type_name, eq(&vehicle_term));
    expect_that!(information.entity_count, eq(1));
    let parked_iri: Uri = format!("http://example.org/{namespace}/isParked").parse()?;
    let parked = information.attribute_details.iter().find(|item| item.id == parked_iri).or_fail()?;
    expect_that!(parked.attribute_name, eq(&format!("isParked_{namespace}").parse::<AttributeName>()?));
    expect_that!(
        parked.attribute_types.as_deref().and_then(|kinds| kinds.first()),
        some(eq(&AttributeKind::Relationship))
    );
    expect_that!(parked.attribute_types.as_deref().and_then(|kinds| kinds.get(1)), none());
    let location_iri: Uri = "https://uri.etsi.org/ngsi-ld/location".parse()?;
    let location = information.attribute_details.iter().find(|item| item.id == location_iri).or_fail()?;
    expect_that!(location.attribute_name, eq(&"location".parse::<AttributeName>()?));
    let status_iri: Uri = format!("http://example.org/{namespace}/status").parse()?;
    let status = information.attribute_details.iter().find(|item| item.id == status_iri).or_fail()?;
    expect_that!(
        status.attribute_name,
        eq(&format!("http://example.org/{namespace}/status").parse::<AttributeName>()?)
    );
    Ok(())
}
