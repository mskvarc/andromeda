//! The attribute information pairs a short name with a full uri.

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

/// Pins: Retrieve Attribute Information under a declaring @context answers for the declared Relationship term its
/// full IRI as id, the term as `attributeName`, Relationship as its one `attributeType` and `attributeCount` 1; for
/// location its full URI and the name location; and for an undeclared attribute addressed by its full IRI that IRI
/// as id and name
/// Clause: 5.7.10, Table 5.2.28-1
#[gtest]
#[tokio::test]
async fn n_dsc_ati_9001_the_attribute_information_pairs_a_short_name_with_a_full_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/declaring.jsonld")?))?;
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
    let is_parked = format!("isParked_{namespace}").parse::<AttributeName>()?;
    let parked = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .attributes()
        .retrieve(&is_parked)
        .send()
        .await?;
    let parked_iri: Uri = format!("http://example.org/{namespace}/isParked").parse()?;
    expect_that!(parked.id, eq(&parked_iri));
    expect_that!(parked.attribute_name, eq(&is_parked));
    expect_that!(parked.attribute_types.as_deref(), some(elements_are![eq(&AttributeKind::Relationship)]));
    expect_that!(parked.attribute_count, some(eq(1)));
    let location = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .attributes()
        .retrieve(&"location".parse::<AttributeName>()?)
        .send()
        .await?;
    let location_iri: Uri = "https://uri.etsi.org/ngsi-ld/location".parse()?;
    expect_that!(location.id, eq(&location_iri));
    expect_that!(location.attribute_name, eq(&"location".parse::<AttributeName>()?));
    let status_name = format!("http://example.org/{namespace}/status").parse::<AttributeName>()?;
    let status = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .attributes()
        .retrieve(&status_name)
        .send()
        .await?;
    let status_iri: Uri = format!("http://example.org/{namespace}/status").parse()?;
    expect_that!(status.id, eq(&status_iri));
    expect_that!(status.attribute_name, eq(&status_name));
    Ok(())
}
