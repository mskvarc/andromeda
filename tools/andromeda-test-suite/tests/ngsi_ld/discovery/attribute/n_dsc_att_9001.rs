//! The attribute list names every declared term short.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Context,
        Entity,
        EntityTypeName,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Available Attributes under an @context declaring a plain term and an @type @id term answers an
/// `AttributeList` naming both short, location short, and an undeclared attribute by its full IRI
/// Clause: 5.7.8, 5.2.27, Table 5.2.28-1
#[gtest]
#[tokio::test]
async fn n_dsc_att_9001_the_attribute_list_names_every_declared_term_short() -> Result<()> {
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
    let attributes = linked.with_accept(Accept::new(NgsiLdMediaType::Json)).attributes().list().send().await?;
    expect_that!(
        attributes.attribute_list,
        contains(eq(&format!("isParked_{namespace}").parse::<AttributeName>()?))
    );
    expect_that!(attributes.attribute_list, contains(eq(&format!("speed_{namespace}").parse::<AttributeName>()?)));
    expect_that!(attributes.attribute_list, contains(eq(&"location".parse::<AttributeName>()?)));
    expect_that!(
        attributes.attribute_list,
        contains(eq(&format!("http://example.org/{namespace}/status").parse::<AttributeName>()?))
    );
    Ok(())
}
