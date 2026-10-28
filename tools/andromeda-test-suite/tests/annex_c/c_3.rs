//! Annex C.3: the context source registration the annex prints, registered and retrieved (clauses 5.2.9 and 5.9.2).

use crate::{
    printed_document::as_floats,
    served_context::{ServedContexts, linked, ok_body},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    model::{
        DateTime,
        Input,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo, TimeInterval},
};
use serde_json::json;

/// The square polygon of the example's location.
fn location() -> Result<Shape> {
    let ring = [(100.0, 0.0), (101.0, 0.0), (101.0, 1.0), (100.0, 1.0), (100.0, 0.0)]
        .into_iter()
        .map(|(longitude, latitude)| Ok(Position::try_new(longitude, latitude)?))
        .collect::<Result<Vec<_>>>()?;
    Ok(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(ring)?])?))
}

/// The id of the example's registration.
fn registration_id(namespace: &Namespace) -> Result<RegistrationId> {
    Ok(namespace.urn_text("ContextSourceRegistration", "csr1a3456").parse()?)
}

/// The registration of the example.
fn registration(namespace: &Namespace) -> Result<CSourceRegistration<Input>> {
    let vehicle = RegistrationInfo::builder()
        .entities([EntityInfo::new(namespace.entity_type("Vehicle")?).with_id(namespace.urn("Vehicle", "A456")?)])
        .property_names(["brandName", "speed"])
        .relationship_names(["isParked"])
        .build()?;
    let parkings = RegistrationInfo::builder()
        .entities([
            EntityInfo::new(namespace.entity_type("OffStreetParking")?).with_id_pattern(".*downtown$".parse()?),
            EntityInfo::new(namespace.entity_type("OffStreetParking")?).with_id_pattern(".*47$".parse()?),
        ])
        .property_names(["availableSpotNumber", "totalSpotNumber"])
        .relationship_names(["isNextToBuilding"])
        .build()?;
    Ok(CSourceRegistration::builder("http://my.csource.org:1026")
        .id(registration_id(namespace)?)
        .information([vehicle, parkings])
        .location(location()?)
        .management_interval(TimeInterval::starting_at("2017-11-29T14:53:15Z".parse::<DateTime>()?))
        .build()?)
}

/// Pins: The registration C.3 prints is created and retrieved with every member it prints; the members 5.2.9
/// defaults or reports and the empty path RFC 3986 makes equivalent to none are not compared
/// Clause: C.3, 5.2.9, 5.9.2, 5.10.1
#[gtest]
#[tokio::test]
async fn c_3_0001_the_context_source_registration_round_trips() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let id = registration_id(&namespace)?;
    let _registration = Cleanup::of(&broker).registration_of(&registration(&namespace)?);
    broker.registrations().register(&registration(&namespace)?).send().await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .registrations()
            .retrieve(&id)
            .send()
            .await,
    )?;
    expect_that!(
        body.get("endpoint").and_then(serde_json::Value::as_str),
        some(matches_regex("^http://my[.]csource[.]org:1026/?$"))
    );
    expect_that!(body.get("id"), some(eq(&json!(id.as_str()))));
    expect_that!(body.get("type"), some(eq(&json!("ContextSourceRegistration"))));
    let vehicle = namespace.suffixed("Vehicle");
    let parking = namespace.suffixed("OffStreetParking");
    expect_that!(
        body.get("information"),
        some(eq(&json!([
            {
                "entities": [{"id": namespace.urn_text("Vehicle", "A456"), "type": vehicle}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"],
            },
            {
                "entities": [{"idPattern": ".*downtown$", "type": parking}, {"idPattern": ".*47$", "type": parking}],
                "propertyNames": ["availableSpotNumber", "totalSpotNumber"],
                "relationshipNames": ["isNextToBuilding"],
            },
        ])))
    );
    expect_that!(
        body.get("location").map(as_floats),
        some(eq(&json!({
            "type": "Polygon",
            "coordinates": [[[100.0, 0.0], [101.0, 0.0], [101.0, 1.0], [100.0, 1.0], [100.0, 0.0]]],
        })))
    );
    expect_that!(body.get("managementInterval"), some(eq(&json!({"startAt": "2017-11-29T14:53:15Z"}))));
    broker.registrations().delete(&id).send().await?;
    served.stop();
    Ok(())
}
