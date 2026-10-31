//! The attribute details pair a short name with a full uri.

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
        Uri,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Available Attributes with details=true under a declaring @context answers the declared
/// Relationship with its full IRI as id, its term as `attributeName` and the declared type term as its one
/// `typeName`, location with its full URI as id and location as name, and an undeclared attribute named by its full
/// IRI
/// Clause: 5.7.8, Table 5.2.28-1
/// Tenant: private (the unfiltered attribute list reads every entity of the tenant, and another file's attribute
/// named `ngsildproof`, which the model refuses, breaks the read)
#[gtest]
#[tokio::test]
async fn n_dsc_atd_9001_the_attribute_details_pair_a_short_name_with_a_full_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/declaring.jsonld")?))?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
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
    let attribute_details = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .attributes()
        .list_details()
        .send()
        .await?;
    let detailed_id: Uri = format!("http://example.org/{namespace}/isParked").parse()?;
    let detailed = attribute_details.iter().find(|item| item.id == detailed_id).or_fail()?;
    expect_that!(detailed.attribute_name, eq(&format!("isParked_{namespace}").parse::<AttributeName>()?));
    expect_that!(detailed.type_names.as_deref().and_then(|names| names.first()), some(eq(&vehicle_type)));
    expect_that!(detailed.type_names.as_deref().and_then(|names| names.get(1)), none());
    let detailed_id_2: Uri = "https://uri.etsi.org/ngsi-ld/location".parse()?;
    let detailed_2 = attribute_details.iter().find(|item| item.id == detailed_id_2).or_fail()?;
    expect_that!(detailed_2.attribute_name, eq(&"location".parse::<AttributeName>()?));
    let detailed_id_3: Uri = "ngsi-ld:location".parse()?;
    expect_that!(attribute_details.iter().find(|item| item.id == detailed_id_3), none());
    let detailed_id_4: Uri = format!("http://example.org/{namespace}/status").parse()?;
    let detailed_3 = attribute_details.iter().find(|item| item.id == detailed_id_4).or_fail()?;
    expect_that!(
        detailed_3.attribute_name,
        eq(&format!("http://example.org/{namespace}/status").parse::<AttributeName>()?)
    );
    Ok(())
}
