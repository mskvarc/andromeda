//! Under a context without the terms every user name is fully qualified.

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

/// Pins: Under an @context declaring none of the entity's terms, Retrieve Available Entity Types names the type by
/// its full IRI, Retrieve Available Attributes names the user attributes by their full IRIs and location short, and
/// with details=true the Relationship record names itself and its type by their full IRIs while location keeps its
/// full URI as id
/// Clause: C.7, 5.7.5, 5.7.8, Table 5.2.28-1
/// Tenant: private (the unfiltered attribute list reads every entity of the tenant, and another file's attribute
/// named `ngsildproof`, which the model refuses, breaks the read)
#[gtest]
#[tokio::test]
async fn n_dsc_atd_9002_under_a_context_without_the_terms_every_user_name_is_fully_qualified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/silent.jsonld")?))?;
    let vehicle_type = format!("http://example.org/{namespace}/Vehicle").parse::<EntityTypeName>()?;
    contexts
        .serve(
            "ctx/silent.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "unrelated_{namespace}": "http://example.org/unrelated/{namespace}"
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
    let types = linked.with_accept(Accept::new(NgsiLdMediaType::Json)).entity_types().list().send().await?;
    expect_that!(types.type_list, contains(eq(&vehicle_type)));
    let attributes = linked.with_accept(Accept::new(NgsiLdMediaType::Json)).attributes().list().send().await?;
    expect_that!(
        attributes.attribute_list,
        contains(eq(&format!("http://example.org/{namespace}/isParked").parse::<AttributeName>()?))
    );
    expect_that!(
        attributes.attribute_list,
        contains(eq(&format!("http://example.org/{namespace}/speed").parse::<AttributeName>()?))
    );
    expect_that!(attributes.attribute_list, contains(eq(&"location".parse::<AttributeName>()?)));
    let attribute_details = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .attributes()
        .list_details()
        .send()
        .await?;
    let detailed_id: Uri = format!("http://example.org/{namespace}/isParked").parse()?;
    let detailed = attribute_details.iter().find(|item| item.id == detailed_id).or_fail()?;
    expect_that!(
        detailed.attribute_name,
        eq(&format!("http://example.org/{namespace}/isParked").parse::<AttributeName>()?)
    );
    expect_that!(detailed.type_names.as_deref().and_then(|names| names.first()), some(eq(&vehicle_type)));
    expect_that!(detailed.type_names.as_deref().and_then(|names| names.get(1)), none());
    let detailed_id_2: Uri = "https://uri.etsi.org/ngsi-ld/location".parse()?;
    let detailed_2 = attribute_details.iter().find(|item| item.id == detailed_id_2).or_fail()?;
    expect_that!(detailed_2.attribute_name, eq(&"location".parse::<AttributeName>()?));
    Ok(())
}
